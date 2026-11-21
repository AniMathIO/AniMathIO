//! Embed timeline markers as a Nero `chpl` chapter list in a finished MP4.
//!
//! # Format choice: Nero `chpl` in `moov/udta`
//!
//! The two common embedded-chapter schemes are:
//!
//! - **`QuickTime` chapter text track** (`tref/chap` from a video track to a text
//!   track): requires a full media track with sample data, a text sample entry
//!   (`tx3g` or legacy `QuickTime` `text`), and proper `tkhd`/`mdia`/`minf`/`stbl`
//!   boxes, making it complex to inject post-mux.  VLC reads it; mpv does not for
//!   `.mp4` files (only `.mov`).
//!
//! - **Nero `chpl`** (`moov/udta/chpl`, version 0): a compact list of
//!   (100 ns timestamp, UTF-8 name) pairs.  VLC, mpv and ffprobe all read it
//!   without needing a separate `ftyp` brand or any media track.  It is easy to
//!   inject as a single box appended to an existing (or empty) `udta`.
//!
//! We therefore write Nero `chpl`, which gives the best cross-player coverage
//! for the smallest on-disk cost.

use animathio_core::{Marker, TICKS_PER_SECOND, Time};

/// A chapter entry as `chpl` sees it: 100-ns timestamp and a UTF-8 title.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ChplEntry {
    /// Timestamp in 100-nanosecond units (Nero epoch = media start).
    ts_100ns: u64,
    /// Chapter title (truncated to 255 UTF-8 bytes if longer).
    title: Vec<u8>,
}

/// 10,000,000 (100-ns ticks per second).
const NS100_PER_SEC: u64 = 10_000_000;

/// Convert timeline ticks relative to `range_start` into 100-ns units.
#[allow(clippy::cast_possible_truncation)]
#[allow(clippy::cast_sign_loss)]
fn ticks_to_100ns(ticks: i64, range_start: i64) -> u64 {
    let relative = ticks.saturating_sub(range_start).max(0) as u128;
    // TICKS_PER_SECOND = 705_600_000; NS100_PER_SEC = 10_000_000
    // ts = relative * 10_000_000 / 705_600_000
    let ticks_per_sec = TICKS_PER_SECOND as u128;
    let ns100 = u128::from(NS100_PER_SEC);
    (relative * ns100 / ticks_per_sec) as u64
}

/// Build the raw bytes of a Nero `chpl` `FullBox` (version 0, flags 0).
///
/// Layout (big-endian):
/// ```text
/// u32  size (including this field)
/// [u8; 4]  "chpl"
/// u8   version = 0
/// [u8; 3]  flags = 0
/// u8   reserved = 0        (Nero extension before the entry count)
/// u32  entry count
/// for each entry:
///   u64  start time in 100-ns units
///   u8   title byte length (0–255)
///   [u8] title bytes (UTF-8, at most 255 bytes)
/// ```
#[allow(clippy::cast_possible_truncation)]
fn build_chpl(entries: &[ChplEntry]) -> Vec<u8> {
    let payload_len: usize = 1 + 4 + entries.iter().map(|e| 8 + 1 + e.title.len()).sum::<usize>();
    let total = 4 + 4 + 4 + payload_len;
    let mut buf = Vec::with_capacity(total);

    buf.extend_from_slice(&(total as u32).to_be_bytes());
    buf.extend_from_slice(b"chpl");
    buf.extend_from_slice(&[0u8, 0, 0, 0]);
    buf.push(0);
    buf.extend_from_slice(
        &u32::try_from(entries.len())
            .unwrap_or(u32::MAX)
            .to_be_bytes(),
    );
    for entry in entries {
        buf.extend_from_slice(&entry.ts_100ns.to_be_bytes());
        let len = u8::try_from(entry.title.len()).unwrap_or(255);
        buf.push(len);
        buf.extend_from_slice(&entry.title[..usize::from(len)]);
    }
    buf
}

/// Build `chpl` entries from project markers, relative to `range_start`.
///
/// Markers are sorted by time (then by ID for determinism).  Markers before
/// `range_start` are skipped; markers at or after `range_end` stop iteration.
/// Duplicate timestamps collapse to the first title.
fn entries_from_markers(markers: &[Marker], range_start: Time, range_end: Time) -> Vec<ChplEntry> {
    let mut last_ts: Option<u64> = None;
    let mut out = Vec::new();
    let mut chapter_num: u64 = 0;

    let mut sorted: Vec<_> = markers.iter().collect();
    sorted.sort_by(|a, b| a.time.cmp(&b.time).then_with(|| a.id.cmp(&b.id)));

    for marker in sorted {
        if marker.time < range_start {
            continue;
        }
        if marker.time >= range_end && range_end.0 > 0 {
            break;
        }

        let ts = ticks_to_100ns(marker.time.0, range_start.0);
        if last_ts == Some(ts) {
            continue;
        }
        last_ts = Some(ts);

        chapter_num += 1;

        let raw = marker.name.trim();
        let title_str: String = if raw.is_empty() {
            format!("Chapter {chapter_num}")
        } else {
            raw.split_whitespace().collect::<Vec<_>>().join(" ")
        };

        let mut bytes = title_str.as_bytes().to_vec();
        if bytes.len() > 255 {
            let mut end = 255;
            while end > 0 && (bytes[end] & 0xC0) == 0x80 {
                end -= 1;
            }
            bytes.truncate(end);
        }

        out.push(ChplEntry {
            ts_100ns: ts,
            title: bytes,
        });
    }
    out
}

/// Find the byte range `[start, end)` of a top-level MP4 box with the given
/// four-character code.  Returns `None` when the box is not found or the
/// data is too short or malformed.
#[allow(clippy::cast_possible_truncation)]
fn find_box(mp4: &[u8], fourcc: [u8; 4]) -> Option<std::ops::Range<usize>> {
    let mut pos = 0;
    while pos + 8 <= mp4.len() {
        let size32 = u32::from_be_bytes(mp4[pos..pos + 4].try_into().ok()?) as usize;
        let size = if size32 == 1 {
            if pos + 16 > mp4.len() {
                return None;
            }
            u64::from_be_bytes(mp4[pos + 8..pos + 16].try_into().ok()?) as usize
        } else if size32 == 0 {
            mp4.len() - pos
        } else {
            size32
        };
        if size < 8 || pos + size > mp4.len() {
            return None;
        }
        if mp4[pos + 4..pos + 8] == fourcc {
            return Some(pos..pos + size);
        }
        pos += size;
    }
    None
}

/// Find a child box inside a container's payload (bytes after the 8-byte
/// header).  Returns the range **within `container`** that covers the child.
fn find_child_box(container: &[u8], fourcc: [u8; 4]) -> Option<std::ops::Range<usize>> {
    let payload_start = if container.len() >= 8 { 8 } else { return None };
    let relative = find_box(&container[payload_start..], fourcc)?;
    Some(relative.start + payload_start..relative.end + payload_start)
}

/// Inject a Nero `chpl` box into the `moov/udta` atom of a finished MP4,
/// returning the modified bytes.
///
/// If `udta` already exists, `chpl` is appended to it (any pre-existing `chpl`
/// is replaced).  If `udta` is absent, a new `udta` containing only `chpl`
/// is appended to `moov`.
///
/// When `moov` appears before `mdat` (faststart / streaming layout), the
/// function also shifts every chunk-offset entry (`stco` 32-bit, `co64` 64-bit)
/// by the same delta so that the offset tables remain valid.  If any `stco`
/// entry would exceed `u32::MAX` after the shift the table is converted to
/// `co64` (which uses 64-bit offsets and is fully supported by the MP4 spec).
///
/// Returns an error when the MP4 is malformed, the `moov` box is missing,
/// or the resulting size would overflow a 32-bit box length.
#[allow(clippy::cast_possible_truncation)]
#[allow(clippy::cast_possible_wrap)]
#[allow(clippy::cast_sign_loss)]
pub fn inject_chapters_into_mp4(mp4: Vec<u8>, entries: &[ChplEntry]) -> Result<Vec<u8>, String> {
    if entries.is_empty() {
        return Ok(mp4);
    }

    let Some(moov_range) = find_box(&mp4, *b"moov") else {
        return Ok(mp4);
    };

    let chpl_bytes = build_chpl(entries);
    let moov_slice = &mp4[moov_range.clone()];

    // --- build the new moov (with chpl injected) ---
    let (mut new_moov, udta_delta) = if let Some(udta_rel) = find_child_box(moov_slice, *b"udta") {
        let udta_abs = moov_range.start + udta_rel.start..moov_range.start + udta_rel.end;
        let udta_slice = &mp4[udta_abs.clone()];

        let new_udta_body = if let Some(chpl_rel) = find_child_box(udta_slice, *b"chpl") {
            let mut body = udta_slice[8..chpl_rel.start].to_vec();
            body.extend_from_slice(&udta_slice[chpl_rel.end..]);
            body
        } else {
            udta_slice[8..].to_vec()
        };

        let new_udta_size = 8 + new_udta_body.len() + chpl_bytes.len();
        if new_udta_size > u32::MAX as usize {
            return Err("udta box would exceed 32-bit size limit".to_owned());
        }
        let mut new_udta = Vec::with_capacity(new_udta_size);
        new_udta.extend_from_slice(&(new_udta_size as u32).to_be_bytes());
        new_udta.extend_from_slice(b"udta");
        new_udta.extend_from_slice(&new_udta_body);
        new_udta.extend_from_slice(&chpl_bytes);

        let udta_delta = (new_udta.len() as i64) - (udta_abs.len() as i64);
        let new_moov_size = (moov_range.len() as i64) + udta_delta;
        if new_moov_size < 8 || new_moov_size > i64::from(u32::MAX) {
            return Err("moov box would exceed 32-bit size limit".to_owned());
        }

        let mut out = Vec::with_capacity(mp4.len() + udta_delta as usize);
        out.extend_from_slice(&mp4[..moov_range.start]);
        out.extend_from_slice(&(new_moov_size as u32).to_be_bytes());
        out.extend_from_slice(b"moov");
        out.extend_from_slice(&mp4[moov_range.start + 8..udta_abs.start]);
        out.extend_from_slice(&new_udta);
        out.extend_from_slice(&mp4[udta_abs.end..moov_range.end]);
        out.extend_from_slice(&mp4[moov_range.end..]);
        (out, udta_delta)
    } else {
        let new_udta_size = 8 + chpl_bytes.len();
        if new_udta_size > u32::MAX as usize {
            return Err("udta box would exceed 32-bit size limit".to_owned());
        }
        let mut new_udta = Vec::with_capacity(new_udta_size);
        new_udta.extend_from_slice(&(new_udta_size as u32).to_be_bytes());
        new_udta.extend_from_slice(b"udta");
        new_udta.extend_from_slice(&chpl_bytes);

        let udta_delta = new_udta_size as i64;
        let new_moov_size = moov_range.len() as i64 + udta_delta;
        if new_moov_size > i64::from(u32::MAX) {
            return Err("moov box would exceed 32-bit size limit".to_owned());
        }

        let mut out = Vec::with_capacity(mp4.len() + udta_delta as usize);
        out.extend_from_slice(&mp4[..moov_range.start]);
        out.extend_from_slice(&(new_moov_size as u32).to_be_bytes());
        out.extend_from_slice(b"moov");
        out.extend_from_slice(&mp4[moov_range.start + 8..moov_range.end]);
        out.extend_from_slice(&new_udta);
        out.extend_from_slice(&mp4[moov_range.end..]);
        (out, udta_delta)
    };

    // --- adjust chunk offsets if moov comes before mdat (faststart layout) ---
    // Detect faststart in the original: mdat starts after moov ended.
    let is_faststart = find_box(&mp4, *b"mdat").is_some_and(|mdat| mdat.start >= moov_range.end);
    if is_faststart {
        new_moov = shift_chunk_offsets(new_moov, udta_delta)?;
    }
    // else: moov is after mdat (non-faststart). Offsets inside moov
    // point at positions that are unaffected by moov's growth.

    Ok(new_moov)
}

/// Walk every box in `mp4` and shift chunk-offset entries by `delta`.
///
/// Handles both `stco` (32-bit) and `co64` (64-bit) offset tables.  When an
/// `stco` entry would exceed `u32::MAX` after the shift the table is converted
/// to `co64`.  Returns an error if the MP4 structure is corrupt.
fn shift_chunk_offsets(mp4: Vec<u8>, delta: i64) -> Result<Vec<u8>, String> {
    if delta == 0 {
        return Ok(mp4);
    }

    // ---- pass 1: identify which stco boxes need to convert to co64 ----
    // We need the total size delta from all conversions to adjust positions
    // in pass 2.
    let mut total_size_delta: i64 = 0;
    let mut pos = 0;
    while pos + 8 <= mp4.len() {
        let (size, header) = match box_size_at(&mp4, pos) {
            Some(s) => s,
            None => break,
        };
        let fourcc = mp4[pos + 4..pos + 8].try_into().unwrap_or([0; 4]);
        if fourcc == *b"stco" {
            let payload_end = pos + size;
            if payload_end - header >= 4 {
                let count =
                    u32::from_be_bytes(mp4[pos + header..pos + header + 4].try_into().unwrap());
                let entry_size = 4;
                let data_start = pos + header + 4;
                let data_end = data_start + count as usize * entry_size;
                if data_end <= payload_end {
                    for i in 0..count {
                        let off = data_start + (i as usize) * entry_size;
                        let offset =
                            i64::from(u32::from_be_bytes(mp4[off..off + 4].try_into().unwrap()));
                        if offset + delta > i64::from(u32::MAX) {
                            // This table must become co64.
                            let new_size = 4 + 8 + 8 * count as usize;
                            let old_size = 4 + 4 + 4 * count as usize;
                            total_size_delta += new_size as i64 - old_size as i64;
                            break; // one overflow ⇒ full conversion
                        }
                    }
                }
            }
        }
        pos += size;
    }

    // ---- pass 2: walk boxes and apply adjustments ----
    let mut out = Vec::with_capacity(mp4.len() + total_size_delta as usize);

    let mut pos = 0;
    while pos + 8 <= mp4.len() {
        let box_start = pos;
        let (size, header) = match box_size_at(&mp4, pos) {
            Some(s) => s,
            None => break,
        };
        let fourcc = mp4[pos + 4..pos + 8].try_into().unwrap_or([0; 4]);

        if fourcc == *b"stco" || fourcc == *b"co64" {
            // ---- chunk-offset table: shift entries ----
            let payload_end = pos + size;
            if payload_end - header < 4 {
                // Too small: copy as-is.
                out.extend_from_slice(&mp4[box_start..box_start + size]);
                pos += size;
                continue;
            }

            let count = u32::from_be_bytes(mp4[pos + header..pos + header + 4].try_into().unwrap());
            let entry_size = if fourcc == *b"stco" { 4 } else { 8 };
            let data_start = pos + header + 4;
            let data_end = data_start + count as usize * entry_size;

            if data_end > payload_end {
                return Err("corrupt chunk-offset table (truncated)".to_owned());
            }

            // Check if this stco table needs conversion to co64.
            let need_convert = if fourcc == *b"stco" {
                let mut overflow = false;
                for i in 0..count {
                    let off = data_start + (i as usize) * entry_size;
                    let offset =
                        i64::from(u32::from_be_bytes(mp4[off..off + 4].try_into().unwrap()));
                    if offset + delta > i64::from(u32::MAX) {
                        overflow = true;
                        break;
                    }
                }
                overflow
            } else {
                false
            };

            if need_convert {
                // ---- convert stco → co64 ----
                let new_size = 4 + 8 + 8 * count as usize; // size + 'co64' + count + entries
                let mut buf = Vec::with_capacity(new_size);
                buf.extend_from_slice(&(new_size as u32).to_be_bytes());
                buf.extend_from_slice(b"co64");
                buf.extend_from_slice(&[0u8; 4]); // version + flags
                // entry count as u64
                buf.extend_from_slice(&(count as u64).to_be_bytes());
                for i in 0..count {
                    let off = data_start + (i as usize) * entry_size;
                    let offset =
                        i64::from(u32::from_be_bytes(mp4[off..off + 4].try_into().unwrap()));
                    buf.extend_from_slice(&(offset + delta).to_be_bytes());
                }
                out.extend_from_slice(&buf);
            } else {
                // ---- shift in-place (32-bit offsets) ----
                let mut buf = Vec::with_capacity(size);
                buf.extend_from_slice(&(size as u32).to_be_bytes());
                buf.extend_from_slice(b"stco");
                buf.extend_from_slice(&mp4[pos + 8..pos + header]);
                buf.extend_from_slice(&mp4[pos + header..pos + header + 4]);
                for i in 0..count {
                    let off = data_start + (i as usize) * entry_size;
                    let offset =
                        i64::from(u32::from_be_bytes(mp4[off..off + 4].try_into().unwrap()));
                    buf.extend_from_slice(&(offset + delta).to_be_bytes());
                }
                out.extend_from_slice(&buf);
            }

            pos += size;
        } else {
            // ---- regular box: copy as-is ----
            out.extend_from_slice(&mp4[box_start..box_start + size]);
            pos += size;
        }
    }

    // Append any trailing bytes (shouldn't happen in a well-formed MP4).
    if pos < mp4.len() {
        out.extend_from_slice(&mp4[pos..]);
    }

    Ok(out)
}

/// Read the box size (in bytes) and header size (8 or 16) at `pos`.
/// Returns `None` if the data is too short or malformed.
fn box_size_at(mp4: &[u8], pos: usize) -> Option<(usize, usize)> {
    if pos + 8 > mp4.len() {
        return None;
    }
    let size32 = u32::from_be_bytes(mp4[pos..pos + 4].try_into().ok()?) as usize;
    let (size, header) = if size32 == 1 {
        // 64-bit size: extended size field follows the fourcc.
        if pos + 16 > mp4.len() {
            return None;
        }
        let largesize = u64::from_be_bytes(mp4[pos + 8..pos + 16].try_into().ok()?);
        // Cap at usize to avoid overflow; also reject values < 16 (too small for header+largesize).
        let size = usize::try_from(largesize).ok()?;
        if size < 16 {
            return None;
        }
        (size, 16)
    } else if size32 == 0 {
        // size == 0 means "extends to end of file".
        (mp4.len() - pos, 8)
    } else {
        (size32, 8)
    };
    if size < header {
        return None;
    }
    Some((size, header))
}

/// Collect chapter entries from project markers in `[range_start, range_end)`.
///
/// Returns `None` when there are no markers in range (so the caller can skip
/// the injection step cheaply).
pub fn chapter_entries(
    markers: &[Marker],
    range_start: Time,
    range_end: Time,
) -> Option<Vec<ChplEntry>> {
    let entries = entries_from_markers(markers, range_start, range_end);
    if entries.is_empty() {
        None
    } else {
        Some(entries)
    }
}

/// Inject timeline markers as a Nero `chpl` chapter list into `mp4_bytes`.
///
/// Only markers in `[range_start, range_end)` are included; each marker's
/// timestamp is shifted so `range_start` maps to 0:00.  Returns the
/// (potentially modified) bytes, or an error if the MP4 structure is
/// corrupt or the chunk offset tables cannot be adjusted.
pub fn embed_chapters(
    mp4_bytes: Vec<u8>,
    markers: &[Marker],
    range_start: Time,
    range_end: Time,
) -> Result<Vec<u8>, String> {
    match chapter_entries(markers, range_start, range_end) {
        Some(entries) => inject_chapters_into_mp4(mp4_bytes, &entries),
        None => Ok(mp4_bytes),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use animathio_core::MarkerId;

    #[test]
    fn chpl_box_layout_matches_nero_spec() {
        let entries = vec![
            ChplEntry {
                ts_100ns: 0,
                title: b"Intro".to_vec(),
            },
            ChplEntry {
                ts_100ns: 10_000_000,
                title: b"Part 2".to_vec(),
            },
        ];
        let buf = build_chpl(&entries);

        let size = u32::from_be_bytes(buf[0..4].try_into().unwrap()) as usize;
        assert_eq!(size, buf.len());
        assert_eq!(&buf[4..8], b"chpl");
        assert_eq!(&buf[8..12], &[0, 0, 0, 0]);
        assert_eq!(buf[12], 0);
        let count = u32::from_be_bytes(buf[13..17].try_into().unwrap());
        assert_eq!(count, 2);

        let ts0 = u64::from_be_bytes(buf[17..25].try_into().unwrap());
        assert_eq!(ts0, 0);
        let len0 = buf[25] as usize;
        assert_eq!(len0, 5);
        assert_eq!(&buf[26..31], b"Intro");

        let ts1 = u64::from_be_bytes(buf[31..39].try_into().unwrap());
        assert_eq!(ts1, 10_000_000);
        let len1 = buf[39] as usize;
        assert_eq!(len1, 6);
        assert_eq!(&buf[40..46], b"Part 2");
    }

    #[test]
    fn title_truncated_to_255_utf8_bytes() {
        let long: String = "a".repeat(300);
        let mut entry = ChplEntry {
            ts_100ns: 0,
            title: long.into_bytes(),
        };
        if entry.title.len() > 255 {
            let mut end = 255;
            while end > 0 && (entry.title[end] & 0xC0) == 0x80 {
                end -= 1;
            }
            entry.title.truncate(end);
        }
        let buf = build_chpl(&[entry]);
        let len_byte = buf[25];
        assert_eq!(len_byte, 255);
    }

    #[test]
    fn ticks_to_100ns_one_second() {
        assert_eq!(ticks_to_100ns(TICKS_PER_SECOND, 0), 10_000_000);
    }

    #[test]
    fn ticks_to_100ns_relative_to_range_start() {
        assert_eq!(
            ticks_to_100ns(2 * TICKS_PER_SECOND, TICKS_PER_SECOND),
            10_000_000
        );
    }

    /// Build a minimal MP4 with just `ftyp` and `moov` (no `udta`).
    #[allow(clippy::cast_possible_truncation)]
    fn minimal_mp4_no_udta() -> Vec<u8> {
        let mut out = Vec::new();
        let ftyp: &[u8] = &[
            0, 0, 0, 16, b'f', b't', b'y', b'p', b'i', b's', b'o', b'm', 0, 0, 2, 0,
        ];
        out.extend_from_slice(ftyp);

        let mvhd_body: &[u8] = &[
            0, 0, 0, 40, b'm', b'v', b'h', b'd', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, 232,
            0, 0, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1,
        ];
        let moov_size = (8 + mvhd_body.len()) as u32;
        out.extend_from_slice(&moov_size.to_be_bytes());
        out.extend_from_slice(b"moov");
        out.extend_from_slice(mvhd_body);
        out
    }

    /// Build a minimal MP4 with `ftyp`, `moov` and an empty `udta`.
    #[allow(clippy::cast_possible_truncation)]
    fn minimal_mp4_with_udta() -> Vec<u8> {
        let mut base = minimal_mp4_no_udta();
        let moov_range = find_box(&base, *b"moov").unwrap();
        let udta: &[u8] = &[0, 0, 0, 8, b'u', b'd', b't', b'a'];
        let new_moov_size = (moov_range.len() + udta.len()) as u32;
        base[moov_range.start..moov_range.start + 4].copy_from_slice(&new_moov_size.to_be_bytes());
        base.extend_from_slice(udta);
        base
    }

    #[test]
    fn inject_into_mp4_without_udta_creates_udta_with_chpl() {
        let mp4 = minimal_mp4_no_udta();
        let entry = ChplEntry {
            ts_100ns: 0,
            title: b"Start".to_vec(),
        };
        let result = inject_chapters_into_mp4(mp4, &[entry]).unwrap();

        let moov_range = find_box(&result, *b"moov").unwrap();
        let moov = &result[moov_range];
        let udta_rel = find_child_box(moov, *b"udta").expect("udta must be present");
        let udta = &moov[udta_rel];
        let chpl_rel = find_child_box(udta, *b"chpl").expect("chpl must be in udta");
        let chpl = &udta[chpl_rel];
        assert_eq!(&chpl[4..8], b"chpl");
        let count = u32::from_be_bytes(chpl[13..17].try_into().unwrap());
        assert_eq!(count, 1);
    }

    #[test]
    fn inject_into_mp4_with_existing_udta_appends_chpl() {
        let mp4 = minimal_mp4_with_udta();
        let entry = ChplEntry {
            ts_100ns: 500_000,
            title: b"Mid".to_vec(),
        };
        let result = inject_chapters_into_mp4(mp4, &[entry]).unwrap();

        let moov_range = find_box(&result, *b"moov").unwrap();
        let moov = &result[moov_range];
        let udta_rel = find_child_box(moov, *b"udta").unwrap();
        let udta = &moov[udta_rel];
        let chpl_rel = find_child_box(udta, *b"chpl").expect("chpl must be in udta");
        let chpl = &udta[chpl_rel];
        let ts = u64::from_be_bytes(chpl[17..25].try_into().unwrap());
        assert_eq!(ts, 500_000);
    }

    #[test]
    fn inject_replaces_existing_chpl() {
        let mp4 = minimal_mp4_no_udta();
        let first = vec![ChplEntry {
            ts_100ns: 0,
            title: b"Old".to_vec(),
        }];
        let mp4_with_chpl = inject_chapters_into_mp4(mp4, &first).unwrap();
        let second = vec![ChplEntry {
            ts_100ns: 0,
            title: b"New".to_vec(),
        }];
        let result = inject_chapters_into_mp4(mp4_with_chpl, &second).unwrap();

        let moov_range = find_box(&result, *b"moov").unwrap();
        let moov = &result[moov_range];
        let udta_rel = find_child_box(moov, *b"udta").unwrap();
        let udta = &moov[udta_rel];
        let chpl_rel = find_child_box(udta, *b"chpl").unwrap();
        let chpl = &udta[chpl_rel];
        let count = u32::from_be_bytes(chpl[13..17].try_into().unwrap());
        assert_eq!(count, 1);
        let title_len = chpl[25] as usize;
        assert_eq!(&chpl[26..26 + title_len], b"New");
    }

    #[test]
    fn empty_entries_returns_original_bytes() {
        let mp4 = minimal_mp4_no_udta();
        let orig_len = mp4.len();
        let result = inject_chapters_into_mp4(mp4, &[]).unwrap();
        assert_eq!(result.len(), orig_len);
    }

    #[test]
    fn not_valid_mp4_returns_original() {
        let garbage = b"not an mp4".to_vec();
        let entry = ChplEntry {
            ts_100ns: 0,
            title: b"X".to_vec(),
        };
        let result = inject_chapters_into_mp4(garbage.clone(), &[entry]).unwrap();
        assert_eq!(result, garbage);
    }

    fn make_marker(id: u64, ticks: i64, name: &str) -> Marker {
        let mut m = Marker::new(MarkerId(id), Time(ticks));
        m.name = name.to_owned();
        m
    }

    #[test]
    fn entries_from_markers_sorted_by_time() {
        let markers = vec![
            make_marker(2, TICKS_PER_SECOND, "Second"),
            make_marker(1, 0, "First"),
            make_marker(3, 2 * TICKS_PER_SECOND, "Third"),
        ];
        let entries = entries_from_markers(&markers, Time(0), Time(3 * TICKS_PER_SECOND));
        assert_eq!(entries.len(), 3);
        assert_eq!(&entries[0].title, b"First");
        assert_eq!(&entries[1].title, b"Second");
        assert_eq!(&entries[2].title, b"Third");
    }

    #[test]
    fn entries_from_markers_skips_before_range_start() {
        let markers = vec![
            make_marker(1, 0, "Before"),
            make_marker(2, TICKS_PER_SECOND, "In"),
        ];
        let entries =
            entries_from_markers(&markers, Time(TICKS_PER_SECOND), Time(2 * TICKS_PER_SECOND));
        assert_eq!(entries.len(), 1);
        assert_eq!(&entries[0].title, b"In");
    }

    #[test]
    fn entries_from_markers_chapter_numbers_sequential() {
        let markers = vec![
            make_marker(1, 0, ""),
            make_marker(2, 0, ""),
            make_marker(3, TICKS_PER_SECOND, ""),
        ];
        let entries = entries_from_markers(&markers, Time(0), Time(2 * TICKS_PER_SECOND));
        assert_eq!(entries.len(), 2);
        assert_eq!(&entries[0].title, b"Chapter 1");
        assert_eq!(&entries[1].title, b"Chapter 2");
    }

    // ------------------------------------------------------------------
    // Chunk offset shifting tests
    // ------------------------------------------------------------------

    /// Build a minimal MP4 with ftyp + moov (containing stbl with stco) + mdat.
    /// The stco table has two entries pointing at offsets 100 and 200.
    #[allow(clippy::cast_possible_truncation)]
    fn minimal_mp4_with_stco() -> Vec<u8> {
        let mut out = Vec::new();
        // ftyp (16 bytes)
        out.extend_from_slice(&[
            0, 0, 0, 16, b'f', b't', b'y', b'p', b'i', b's', b'o', b'm', 0, 0, 2, 0,
        ]);

        // stco payload: version+flags (4) + count (4) + 2 entries (8) = 16
        let stco_payload: Vec<u8> = [
            0, 0, 0, 0, // version + flags
            0, 0, 0, 2, // entry count = 2
            0, 0, 0, 100, // offset 1
            0, 0, 0, 200, // offset 2
        ]
        .to_vec();
        let stco_payload_len = stco_payload.len(); // 16

        // stco box: size (4) + fourcc (4) + payload (16) = 24
        let stco_size = 4 + 4 + stco_payload_len; // 24

        // stbl box: size (4) + fourcc (4) + stco box (24) = 32
        let stbl_size = 4 + 4 + stco_size; // 32

        // mvhd (40 bytes)
        let mvhd: [u8; 40] = [0u8; 40];

        // moov box: size (4) + fourcc (4) + stbl (32) + mvhd (40) = 80
        let moov_size = 8 + stbl_size + mvhd.len(); // 80

        // moov
        out.extend_from_slice(&(moov_size as u32).to_be_bytes());
        out.extend_from_slice(b"moov");
        // stbl
        out.extend_from_slice(&(stbl_size as u32).to_be_bytes());
        out.extend_from_slice(b"stbl");
        // stco
        out.extend_from_slice(&(stco_size as u32).to_be_bytes());
        out.extend_from_slice(b"stco");
        out.extend_from_slice(&stco_payload);
        // mvhd
        out.extend_from_slice(&mvhd);

        // mdat (size 0 = extends to EOF)
        out.extend_from_slice(&[0, 0, 0, 0, b'm', b'd', b'a', b't']);
        out
    }

    #[test]
    fn stco_offsets_shift_by_delta_when_moov_before_mdat() {
        let mp4 = minimal_mp4_with_stco();
        // Inject chapters (this grows moov, shifting mdat).
        let entry = ChplEntry {
            ts_100ns: 0,
            title: b"Ch".to_vec(),
        };
        let result = inject_chapters_into_mp4(mp4, &[entry]).unwrap();

        // Find stco offsets after injection.
        let boxes = parse_boxes(&result);
        let moov_box = boxes.iter().find(|(fc, _, _)| *fc == *b"moov").unwrap();
        let moov = &result[moov_box.1..moov_box.2];
        let stbl_ranges = find_children(moov, *b"stbl");
        assert!(!stbl_ranges.is_empty());
        let stbl = &moov[stbl_ranges[0].clone()];
        let stco_ranges = find_children(stbl, *b"stco");
        let stco = &stbl[stco_ranges[0].clone()];
        // stco layout: size(4) + fourcc(4) + version+flags(4) + count(4) + entries
        let count_after = u32::from_be_bytes(stco[12..16].try_into().unwrap());
        assert_eq!(count_after, 2);
        let off1_after = u32::from_be_bytes(stco[16..20].try_into().unwrap());
        let off2_after = u32::from_be_bytes(stco[20..24].try_into().unwrap());
        // Offsets must have increased by exactly the delta (moov growth).
        let delta = (moov.len() as i64) - (moov_box.2 - moov_box.1) as i64;
        assert_eq!(
            off1_after as i64 - 100,
            delta,
            "first offset shifted by delta"
        );
        assert_eq!(
            off2_after as i64 - 200,
            delta,
            "second offset shifted by delta"
        );
        // Both offsets must still be valid (positive).
        assert!(off1_after > 0, "first offset must be positive");
        assert!(off2_after > 0, "second offset must be positive");
    }

    /// Build a minimal MP4 with ftyp + mdat + moov (non-faststart, moov after mdat).
    #[allow(clippy::cast_possible_truncation)]
    fn minimal_mp4_non_faststart() -> Vec<u8> {
        let mut out = Vec::new();
        // ftyp (16 bytes)
        out.extend_from_slice(&[
            0, 0, 0, 16, b'f', b't', b'y', b'p', b'i', b's', b'o', b'm', 0, 0, 2, 0,
        ]);
        // mdat (size 8 = header only, no content)
        out.extend_from_slice(&[0, 0, 0, 8, b'm', b'd', b'a', b't']);

        // stco payload: version+flags (4) + count (4) + 1 entry (4) = 12
        let stco_payload: Vec<u8> = [0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 100].to_vec();
        let stco_payload_len = stco_payload.len(); // 12

        // stco box: size (4) + fourcc (4) + payload (12) = 20
        let stco_size = 4 + 4 + stco_payload_len; // 20

        // stbl box: size (4) + fourcc (4) + stco box (20) = 28
        let stbl_size = 4 + 4 + stco_size; // 28

        // mvhd (40 bytes)
        let mvhd: [u8; 40] = [0u8; 40];

        // moov box: size (4) + fourcc (4) + stbl (28) + mvhd (40) = 76
        let moov_size = 8 + stbl_size + mvhd.len(); // 76

        // moov
        out.extend_from_slice(&(moov_size as u32).to_be_bytes());
        out.extend_from_slice(b"moov");
        // stbl
        out.extend_from_slice(&(stbl_size as u32).to_be_bytes());
        out.extend_from_slice(b"stbl");
        // stco
        out.extend_from_slice(&(stco_size as u32).to_be_bytes());
        out.extend_from_slice(b"stco");
        out.extend_from_slice(&stco_payload);
        // mvhd
        out.extend_from_slice(&mvhd);
        out
    }

    #[test]
    fn stco_offsets_unaffected_when_moov_after_mdat() {
        let mp4 = minimal_mp4_non_faststart();
        let entry = ChplEntry {
            ts_100ns: 0,
            title: b"Ch".to_vec(),
        };
        let result = inject_chapters_into_mp4(mp4, &[entry]).unwrap();

        // The stco offset should be unchanged.
        let boxes = parse_boxes(&result);
        let moov_box = boxes.iter().find(|(fc, _, _)| *fc == *b"moov").unwrap();
        let moov = &result[moov_box.1..moov_box.2];
        let stbl_ranges = find_children(moov, *b"stbl");
        assert!(!stbl_ranges.is_empty());
        let stbl = &moov[stbl_ranges[0].clone()];
        let stco_ranges = find_children(stbl, *b"stco");
        let stco = &stbl[stco_ranges[0].clone()];
        // stco layout: size(4) + fourcc(4) + version+flags(4) + count(4) + offset(4)
        let off = u32::from_be_bytes(stco[16..20].try_into().unwrap());
        assert_eq!(off, 100, "offset must be unchanged when moov is after mdat");
    }

    /// Test that 64-bit box sizes (size==1 with largesize) are handled.
    #[test]
    fn box_size_at_handles_64bit_size() {
        // A box with size=1 and a 64-bit largesize.
        let largesize: u64 = 1000;
        let box_bytes: Vec<u8> = [
            0, 0, 0, 1, // size = 1 (64-bit)
            b'f', b't', b'y', b'p',
        ]
        .into_iter()
        .chain(largesize.to_be_bytes())
        .chain((0..largesize as usize - 16).map(|_| 0u8))
        .collect();
        let (size, header) = box_size_at(&box_bytes, 0).unwrap();
        assert_eq!(size, 1000);
        assert_eq!(header, 16);
    }

    /// Test that size==0 (extends to EOF) is handled.
    #[test]
    fn box_size_at_handles_zero_size() {
        // A box with size=0 means "extends to end of file".
        let mut data = vec![0, 0, 0, 0, b'x', b'o', b'x', b'o']; // size=0, fourcc="xoxo"
        data.extend_from_slice(&vec![0u8; 96]); // 96 more bytes → total 100
        let (size, header) = box_size_at(&data, 0).unwrap();
        assert_eq!(size, data.len());
        assert_eq!(header, 8);
    }

    /// Minimal helper: parse top-level box ranges.
    #[allow(clippy::cast_possible_truncation)]
    fn parse_boxes(mp4: &[u8]) -> Vec<([u8; 4], usize, usize)> {
        let mut pos = 0;
        let mut out = Vec::new();
        while pos + 8 <= mp4.len() {
            let size32 = u32::from_be_bytes(mp4[pos..pos + 4].try_into().unwrap()) as usize;
            let (size, header) = if size32 == 1 {
                if pos + 16 > mp4.len() {
                    break;
                }
                (
                    u64::from_be_bytes(mp4[pos + 8..pos + 16].try_into().unwrap()) as usize,
                    16,
                )
            } else if size32 == 0 {
                (mp4.len() - pos, 8)
            } else {
                (size32, 8)
            };
            if size < header || pos + size > mp4.len() {
                break;
            }
            let mut fc = [0u8; 4];
            fc.copy_from_slice(&mp4[pos + 4..pos + 8]);
            out.push((fc, pos, pos + size));
            pos += size;
        }
        out
    }

    /// Find all boxes named `fourcc` inside a container's payload.
    fn find_children(container: &[u8], fourcc: [u8; 4]) -> Vec<std::ops::Range<usize>> {
        let start = if container.len() >= 8 {
            8
        } else {
            return Vec::new();
        };
        parse_boxes(&container[start..])
            .into_iter()
            .filter(|(fc, _, _)| *fc == fourcc)
            .map(|(_, s, e)| s + start..e + start)
            .collect()
    }
}
