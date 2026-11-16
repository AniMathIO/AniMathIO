//! Explicit colour handling for decoded frames.
//!
//! FilmCraft already resolves container + bitstream colour metadata, but its default for an
//! *unspecified* matrix is a resolution heuristic (BT.601 for SD, BT.709 otherwise). FFmpeg —
//! and therefore what most users see in other players — defaults to **BT.601** regardless of
//! resolution. We therefore re-resolve the matrix for MP4/MOV ourselves and convert Y'CbCr to
//! RGBA8 explicitly, so unspecified-colour files match FFmpeg.

use std::sync::Arc;

use filmcraft_color as fc;
use filmcraft_frame::{Chroma, PixelData};

/// YUV ↔ RGB matrix coefficients.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Matrix {
    /// BT.601 (SD).
    Bt601,
    /// BT.709 (HD).
    Bt709,
    /// BT.2020 non-constant luminance.
    Bt2020Ncl,
}

/// Transfer characteristics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Transfer {
    /// BT.709 / display gamma.
    Bt709,
    /// sRGB.
    Srgb,
    /// Linear light.
    Linear,
    /// SMPTE ST 2084 (PQ).
    Pq,
    /// ARIB STD-B67 (HLG).
    Hlg,
}

/// Colour primaries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Primaries {
    /// BT.709.
    Bt709,
    /// BT.601 625-line.
    Bt601_625,
    /// BT.601 525-line.
    Bt601_525,
    /// BT.2020.
    Bt2020,
    /// DCI-P3 D65.
    P3D65,
}

/// Luma/chroma value range.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColorRange {
    /// Studio swing (16–235 luma).
    Limited,
    /// Full swing.
    Full,
}

/// Colour metadata attached to a decoded frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ColorInfo {
    /// The matrix used for the explicit Y'CbCr → RGBA conversion.
    pub matrix: Matrix,
    /// Transfer characteristics signalled by (or defaulted for) the stream.
    pub transfer: Transfer,
    /// Primaries signalled by (or defaulted for) the stream.
    pub primaries: Primaries,
    /// Value range signalled by (or defaulted for) the stream.
    pub range: ColorRange,
    /// Whether `matrix` came from the stream itself (`true`) or from our FFmpeg-compatible
    /// BT.601 default (`false`).
    pub matrix_specified: bool,
}

impl From<fc::Matrix> for Matrix {
    fn from(value: fc::Matrix) -> Self {
        match value {
            fc::Matrix::Bt601 => Matrix::Bt601,
            fc::Matrix::Bt709 => Matrix::Bt709,
            fc::Matrix::Bt2020Ncl => Matrix::Bt2020Ncl,
        }
    }
}

impl From<fc::Transfer> for Transfer {
    fn from(value: fc::Transfer) -> Self {
        match value {
            fc::Transfer::Bt709 => Transfer::Bt709,
            fc::Transfer::Srgb => Transfer::Srgb,
            fc::Transfer::Linear => Transfer::Linear,
            fc::Transfer::Pq => Transfer::Pq,
            fc::Transfer::Hlg => Transfer::Hlg,
        }
    }
}

impl From<fc::Primaries> for Primaries {
    fn from(value: fc::Primaries) -> Self {
        match value {
            fc::Primaries::Bt709 => Primaries::Bt709,
            fc::Primaries::Bt601_625 => Primaries::Bt601_625,
            fc::Primaries::Bt601_525 => Primaries::Bt601_525,
            fc::Primaries::Bt2020 => Primaries::Bt2020,
            fc::Primaries::P3D65 => Primaries::P3D65,
        }
    }
}

impl From<fc::Range> for ColorRange {
    fn from(value: fc::Range) -> Self {
        match value {
            fc::Range::Limited => ColorRange::Limited,
            fc::Range::Full => ColorRange::Full,
        }
    }
}

impl ColorInfo {
    /// Build our colour description from FilmCraft's resolved metadata and whether the matrix was
    /// explicitly signalled. The matrix is supplied separately (it may have been re-defaulted).
    pub(crate) fn from_filmcraft(
        resolved: fc::ColorInfo,
        matrix: Matrix,
        matrix_specified: bool,
    ) -> Self {
        Self {
            matrix,
            transfer: resolved.transfer.into(),
            primaries: resolved.primaries.into(),
            range: resolved.range.into(),
            matrix_specified,
        }
    }
}

/// The result of inspecting an MP4/MOV file's colour signalling and sample cadence.
pub(crate) struct Mp4Probe {
    /// True when the video track's sample durations are not all equal.
    pub vfr: bool,
    /// The matrix code signalled by a `colr` box or the codec's VUI, when one is present.
    pub signalled_matrix: Option<fc::Matrix>,
}

/// Inspect MP4/MOV bytes for VFR cadence and an explicitly signalled colour matrix.
///
/// Returns `None` when the file cannot be parsed as MP4/MOV (callers then fall back to
/// FilmCraft's resolved metadata).
pub(crate) fn mp4_probe(bytes: &[u8]) -> Option<Mp4Probe> {
    let mp4 = filmcraft_isobmff::open(bytes).ok()?;
    let track = mp4.track_of_kind(filmcraft_isobmff::TrackKind::Video)?;
    let t = &mp4.tracks[track];

    let vfr = t
        .samples
        .first()
        .is_some_and(|first| t.samples.iter().any(|s| s.duration != first.duration));

    let entry = t.entries.first()?;
    let colr_matrix = entry
        .video
        .as_ref()
        .and_then(|v| v.color.as_ref())
        .and_then(colr_matrix_code);

    let signalled_matrix = colr_matrix.or_else(|| codec_matrix_code(&entry.codec));

    Some(Mp4Probe {
        vfr,
        signalled_matrix,
    })
}

fn colr_matrix_code(color: &filmcraft_isobmff::ColorInfo) -> Option<fc::Matrix> {
    match color {
        filmcraft_isobmff::ColorInfo::Nclx { matrix, .. }
        | filmcraft_isobmff::ColorInfo::Nclc { matrix, .. } => {
            fc::Matrix::from_code(u8::try_from(*matrix).unwrap_or(2))
        }
        filmcraft_isobmff::ColorInfo::Icc { .. } => None,
    }
}

fn codec_matrix_code(codec: &filmcraft_isobmff::CodecConfig) -> Option<fc::Matrix> {
    match codec {
        filmcraft_isobmff::CodecConfig::Avc(cfg) => {
            avc_vui(cfg).and_then(|vui| fc::Matrix::from_code(vui.matrix_coefficients))
        }
        // VP8 has no colour signalling: its one colour space is BT.601 (RFC 6386 §9.2).
        filmcraft_isobmff::CodecConfig::Unknown { fourcc, .. } if fourcc.0 == *b"vp08" => {
            Some(fc::Matrix::Bt601)
        }
        _ => None,
    }
}

fn avc_vui(cfg: &filmcraft_isobmff::AvcConfig) -> Option<filmcraft_h264::params::Vui> {
    let nal = cfg
        .sps
        .iter()
        .find(|n| n.first().is_some_and(|h| h & 0x1f == 7))?;
    let sps =
        filmcraft_h264::params::Sps::parse(&filmcraft_bitstream::unescape_rbsp(nal.get(1..)?))
            .ok()?;
    sps.vui
}

/// Resolve the colour to use for conversion, applying the FFmpeg-compatible BT.601 default when
/// the matrix is unspecified (MP4/MOV), and FilmCraft's resolved matrix otherwise.
pub(crate) fn resolve(bytes: &[u8], resolved: fc::ColorInfo) -> (ColorInfo, bool) {
    if let Some(probe) = mp4_probe(bytes) {
        let matrix = probe.signalled_matrix.map_or(Matrix::Bt601, Matrix::from);
        let specified = probe.signalled_matrix.is_some();
        (
            ColorInfo::from_filmcraft(resolved, matrix, specified),
            probe.vfr,
        )
    } else if bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        let matrix = filmcraft_matroska::open_with(
            bytes,
            &filmcraft_matroska::OpenOptions {
                index: false,
                verify_crc: false,
            },
        )
        .ok()
        .and_then(|file| file.tracks.into_iter().find(|t| t.video.is_some()))
        .and_then(|track| {
            let container = track
                .video
                .as_ref()
                .and_then(|v| v.colour.as_ref())
                .and_then(|c| c.matrix_coefficients)
                .and_then(|c| u8::try_from(c).ok())
                .and_then(fc::Matrix::from_code);
            container.or_else(|| match track.codec {
                filmcraft_matroska::Codec::Avc { avcc } => {
                    filmcraft_isobmff::AvcConfig::parse(&avcc)
                        .ok()
                        .and_then(|cfg| avc_vui(&cfg))
                        .and_then(|vui| fc::Matrix::from_code(vui.matrix_coefficients))
                }
                _ => None,
            })
        });
        (
            ColorInfo::from_filmcraft(
                resolved,
                matrix.map_or(Matrix::Bt601, Matrix::from),
                matrix.is_some(),
            ),
            false,
        )
    } else {
        (
            ColorInfo::from_filmcraft(resolved, Matrix::Bt601, false),
            false,
        )
    }
}

/// `x.round().clamp(0.0, 255.0) as u8` without the libm `round` call (baseline x86-64 has no
/// rounding instruction). After clamping `x` is non-negative, so truncating and testing the
/// exact fractional part reproduces round-half-away-from-zero bit for bit.
#[inline]
pub(crate) fn round_u8(x: f64) -> u8 {
    let x = x.clamp(0.0, 255.0);
    let t = x as u8;
    t + u8::from(x - f64::from(t) >= 0.5)
}

/// Convert an 8-bit planar Y'CbCr frame to straight-alpha RGBA8 using an explicit matrix and
/// range. This is the direct display path (no linearisation), matching FFmpeg's `-pix_fmt`
/// default and FilmCraft's own fast path.
pub(crate) fn yuv8_to_rgba8(
    width: u32,
    height: u32,
    planes: &[Arc<Vec<u8>>; 3],
    chroma: Chroma,
    alpha: Option<&Arc<Vec<u8>>>,
    matrix: Matrix,
    range: ColorRange,
) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let (sx, sy) = chroma.shifts();
    let cw = w.div_ceil(1 << sx);
    let (kr, kb) = kr_kb(matrix);
    let kg = 1.0 - kr - kb;
    let (ys, yo, cs) = match range {
        ColorRange::Limited => (255.0 / 219.0, 16.0, 255.0 / 224.0),
        ColorRange::Full => (1.0, 0.0, 1.0),
    };
    let (crr, cbb) = (2.0 * (1.0 - kr) * cs, 2.0 * (1.0 - kb) * cs);
    let (cgr, cgb) = (crr * kr / kg, cbb * kb / kg);
    // Per-sample terms depend on one byte only, so tabulate them. Every table entry and
    // every sum below uses the same f64 operations in the same order as the direct formula,
    // which keeps the output bit-identical.
    let luma: [f64; 256] = std::array::from_fn(|i| (i as f64 - yo) * ys);
    let chroma_of = |k: f64| -> [f64; 256] { std::array::from_fn(|i| k * (i as f64 - 128.0)) };
    let (r_v, g_v, g_u, b_u) = (
        chroma_of(crr),
        chroma_of(cgr),
        chroma_of(cgb),
        chroma_of(cbb),
    );
    let mut out = vec![0u8; w * h * 4];
    for (y, orow) in out.chunks_exact_mut(w * 4).enumerate() {
        let cy = y >> sy;
        let yrow = &planes[0][y * w..y * w + w];
        let urow = &planes[1][cy * cw..cy * cw + cw];
        let vrow = &planes[2][cy * cw..cy * cw + cw];
        let arow = alpha.map(|a| &a[y * w..y * w + w]);
        for (x, o) in orow.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let yy = luma[usize::from(yrow[x])];
            let (u, v) = (usize::from(urow[x >> sx]), usize::from(vrow[x >> sx]));
            o[0] = round_u8(yy + r_v[v]);
            o[1] = round_u8(yy - g_v[v] - g_u[u]);
            o[2] = round_u8(yy + b_u[u]);
            o[3] = arow.map_or(255, |a| a[x]);
        }
    }
    out
}

/// Convert a decoded [`filmcraft_frame::VideoFrame`] to straight-alpha RGBA8, honouring our
/// explicit colour resolution.
pub(crate) fn frame_to_rgba8(
    frame: &filmcraft_frame::VideoFrame,
    color: ColorInfo,
) -> crate::Result<Vec<u8>> {
    let bytes = crate::limits::frame_bytes(frame.width, frame.height)?;
    let pixels = bytes / 4;
    let planes_valid = |lengths: [usize; 3], chroma: Chroma, alpha: Option<usize>| {
        let (sx, sy) = chroma.shifts();
        let uv =
            (frame.width as usize).div_ceil(1 << sx) * (frame.height as usize).div_ceil(1 << sy);
        lengths[0] >= pixels
            && lengths[1] >= uv
            && lengths[2] >= uv
            && alpha.is_none_or(|n| n >= pixels)
    };
    let valid = match &frame.data {
        PixelData::Rgba8(data) => data.len() == bytes,
        PixelData::RgbaF32(data) => data.len() == bytes,
        PixelData::Yuv8 {
            planes,
            chroma,
            alpha,
        } => planes_valid(
            planes.each_ref().map(|p| p.len()),
            *chroma,
            alpha.as_ref().map(|p| p.len()),
        ),
        PixelData::Yuv16 {
            planes,
            chroma,
            bits,
            alpha,
        } => {
            (9..=16).contains(bits)
                && planes_valid(
                    planes.each_ref().map(|p| p.len()),
                    *chroma,
                    alpha.as_ref().map(|p| p.len()),
                )
        }
    };
    if !valid {
        return Err(crate::MediaError::Decode(
            "truncated decoded frame planes".into(),
        ));
    }
    Ok(match &frame.data {
        PixelData::Rgba8(data) => data.as_ref().clone(),
        PixelData::Yuv8 {
            planes,
            chroma,
            alpha,
        } => yuv8_to_rgba8(
            frame.width,
            frame.height,
            planes,
            *chroma,
            alpha.as_ref(),
            color.matrix,
            color.range,
        ),
        _ => {
            let mut frame = frame.clone();
            frame.color.matrix = match color.matrix {
                Matrix::Bt601 => fc::Matrix::Bt601,
                Matrix::Bt709 => fc::Matrix::Bt709,
                Matrix::Bt2020Ncl => fc::Matrix::Bt2020Ncl,
            };
            frame.to_rgba8()
        }
    })
}

fn kr_kb(matrix: Matrix) -> (f64, f64) {
    match matrix {
        Matrix::Bt601 => (0.299, 0.114),
        Matrix::Bt709 => (0.2126, 0.0722),
        Matrix::Bt2020Ncl => (0.2627, 0.0593),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pre-optimisation scalar formula.
    fn reference(
        w: usize,
        h: usize,
        planes: &[Arc<Vec<u8>>; 3],
        chroma: Chroma,
        matrix: Matrix,
        range: ColorRange,
    ) -> Vec<u8> {
        let (sx, sy) = chroma.shifts();
        let cw = w.div_ceil(1 << sx);
        let (kr, kb) = kr_kb(matrix);
        let kg = 1.0 - kr - kb;
        let (ys, yo, cs) = match range {
            ColorRange::Limited => (255.0 / 219.0, 16.0, 255.0 / 224.0),
            ColorRange::Full => (1.0, 0.0, 1.0),
        };
        let (crr, cbb) = (2.0 * (1.0 - kr) * cs, 2.0 * (1.0 - kb) * cs);
        let (cgr, cgb) = (crr * kr / kg, cbb * kb / kg);
        let mut out = vec![0u8; w * h * 4];
        for y in 0..h {
            let cy = y >> sy;
            for x in 0..w {
                let yy = (f64::from(planes[0][y * w + x]) - yo) * ys;
                let u = f64::from(planes[1][cy * cw + (x >> sx)]) - 128.0;
                let v = f64::from(planes[2][cy * cw + (x >> sx)]) - 128.0;
                let o = &mut out[(y * w + x) * 4..(y * w + x) * 4 + 4];
                o[0] = (yy + crr * v).round().clamp(0.0, 255.0) as u8;
                o[1] = (yy - cgr * v - cgb * u).round().clamp(0.0, 255.0) as u8;
                o[2] = (yy + cbb * u).round().clamp(0.0, 255.0) as u8;
                o[3] = 255;
            }
        }
        out
    }

    #[test]
    fn round_u8_matches_round_clamp() {
        let mut values = vec![
            -1.0,
            0.0,
            0.499_999_999_999_999_94,
            0.5,
            1.5,
            254.5,
            254.999,
            255.0,
            300.0,
        ];
        values.extend((0..200_000).map(|i| f64::from(i) * 0.00137 - 3.0));
        for x in values {
            assert_eq!(round_u8(x), x.round().clamp(0.0, 255.0) as u8, "{x}");
        }
    }

    #[test]
    fn table_driven_yuv_to_rgba_matches_the_direct_formula() {
        let (w, h) = (64usize, 36usize);
        let mut seed = 0x1234_5678_u32;
        let mut next = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 24) as u8
        };
        let planes: [Arc<Vec<u8>>; 3] = [
            Arc::new((0..w * h).map(|_| next()).collect()),
            Arc::new((0..w * h / 4).map(|_| next()).collect()),
            Arc::new((0..w * h / 4).map(|_| next()).collect()),
        ];
        for matrix in [Matrix::Bt601, Matrix::Bt709, Matrix::Bt2020Ncl] {
            for range in [ColorRange::Limited, ColorRange::Full] {
                let actual = yuv8_to_rgba8(
                    w as u32,
                    h as u32,
                    &planes,
                    Chroma::C420,
                    None,
                    matrix,
                    range,
                );
                assert_eq!(
                    actual,
                    reference(w, h, &planes, Chroma::C420, matrix, range)
                );
            }
        }
    }
}
