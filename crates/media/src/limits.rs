//! Resource bounds checked before codec work and before host-side allocations.
use crate::{MediaError, Result};

/// Maximum encoded bytes accepted for one asset (256 MiB).
pub const MAX_INPUT_BYTES: usize = 256 * 1024 * 1024;
/// Maximum width or height accepted from media.
pub const MAX_DIMENSION: u32 = 8192;
/// Maximum decoded pixels in a frame (32 megapixels).
pub const MAX_PIXELS: usize = 32 * 1024 * 1024;
/// Maximum indexed frames/samples per track.
pub const MAX_FRAMES: u64 = 1_000_000;
/// Maximum native audio sample rate.
pub const MAX_SAMPLE_RATE: u32 = 192_000;
/// Maximum interleaved PCM samples in a single request (64 MiB).
pub const MAX_AUDIO_SAMPLES: usize = 16 * 1024 * 1024;

pub(crate) fn invalid() -> MediaError {
    MediaError::Invalid("media resource limit exceeded or truncated metadata".into())
}

pub(crate) fn frame_bytes(width: u32, height: u32) -> Result<usize> {
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(invalid());
    }
    let pixels = usize::try_from(width)
        .ok()
        .and_then(|w| w.checked_mul(height as usize))
        .filter(|n| *n <= MAX_PIXELS)
        .ok_or_else(invalid)?;
    pixels.checked_mul(4).ok_or_else(invalid)
}

pub(crate) fn audio(rate: u32, channels: usize, frames: usize) -> Result<()> {
    if rate == 0
        || rate > MAX_SAMPLE_RATE
        || channels == 0
        || channels > 8
        || frames
            .checked_mul(channels)
            .is_none_or(|n| n > MAX_AUDIO_SAMPLES)
    {
        return Err(invalid());
    }
    Ok(())
}

pub(crate) fn input(bytes: &[u8]) -> Result<()> {
    if bytes.len() > MAX_INPUT_BYTES || bytes.len() < 8 {
        return Err(invalid());
    }
    // Bound MP4's expanded sample tables *before* the demuxer allocates them.
    if &bytes[4..8] == b"ftyp" {
        boxes(bytes, 0)?;
    }
    Ok(())
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_be_bytes(
        bytes
            .get(at..at + 4)
            .ok_or_else(invalid)?
            .try_into()
            .map_err(|_| invalid())?,
    ))
}

fn boxes(mut bytes: &[u8], depth: usize) -> Result<()> {
    if depth > 16 {
        return Err(invalid());
    }
    while !bytes.is_empty() {
        let size = u32_at(bytes, 0)?;
        let kind = bytes.get(4..8).ok_or_else(invalid)?;
        let (size, header) = if size == 1 {
            let n = u64::from_be_bytes(
                bytes
                    .get(8..16)
                    .ok_or_else(invalid)?
                    .try_into()
                    .map_err(|_| invalid())?,
            );
            (usize::try_from(n).map_err(|_| invalid())?, 16)
        } else if size == 0 {
            (bytes.len(), 8)
        } else {
            (size as usize, 8)
        };
        if size < header {
            return Err(invalid());
        }
        let body = bytes.get(header..size).ok_or_else(invalid)?;
        match kind {
            b"moov" | b"trak" | b"mdia" | b"minf" | b"stbl" | b"edts" => boxes(body, depth + 1)?,
            b"stsz" => {
                if u64::from(u32_at(body, 8)?) > MAX_FRAMES {
                    return Err(invalid());
                }
            }
            b"stts" | b"ctts" => {
                let count = u32_at(body, 4)? as usize;
                if count > MAX_FRAMES as usize
                    || count
                        .checked_mul(8)
                        .and_then(|n| n.checked_add(8))
                        .is_none_or(|n| n > body.len())
                {
                    return Err(invalid());
                }
                let mut total = 0u64;
                for i in 0..count {
                    total = total
                        .checked_add(u64::from(u32_at(body, 8 + i * 8)?))
                        .ok_or_else(invalid)?;
                }
                if total > MAX_FRAMES {
                    return Err(invalid());
                }
            }
            b"stco" | b"co64" | b"stsc" | b"stss" | b"stsd"
                if u64::from(u32_at(body, 4)?) > MAX_FRAMES =>
            {
                return Err(invalid());
            }
            _ => {}
        }
        bytes = bytes.get(size..).ok_or_else(invalid)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checked_frame_and_pcm_allocations() {
        for (w, h) in [(0, 1), (u32::MAX, u32::MAX), (8192, 8192)] {
            assert!(frame_bytes(w, h).is_err());
        }
        assert_eq!(frame_bytes(320, 180).unwrap(), 230_400);
        for (rate, channels, frames) in [
            (u32::MAX, 2, 1),
            (48000, usize::MAX, 1),
            (48000, 2, usize::MAX),
            (0, 2, 0),
        ] {
            assert!(audio(rate, channels, frames).is_err());
        }
    }
    #[test]
    fn forged_expansion_and_truncated_boxes_are_rejected() {
        let mut bytes = Vec::from(&b"\0\0\0\x08ftyp\0\0\0\x18stts\0\0\0\0\0\0\0\x01"[..]);
        bytes.extend(u32::MAX.to_be_bytes());
        bytes.extend(1u32.to_be_bytes());
        assert!(input(&bytes).is_err());
        for end in (1..bytes.len()).filter(|n| *n != 8) {
            assert!(input(&bytes[..end]).is_err());
        }
    }
}
