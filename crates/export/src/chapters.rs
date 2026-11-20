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
        &u32::try_from(entries.len()).unwrap_or(u32::MAX).to_be_bytes(),
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

        out.push(ChplEntry { ts_100ns: ts, title: bytes });
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
/// The function is a no-op (returns the original bytes unchanged) when:
/// - `entries` is empty,
/// - the `moov` box cannot be found (the data is not a valid MP4), or
/// - the resulting size would overflow a 32-bit box length.
#[allow(clippy::cast_possible_truncation)]
#[allow(clippy::cast_possible_wrap)]
#[allow(clippy::cast_sign_loss)]
pub fn inject_chapters_into_mp4(mp4: Vec<u8>, entries: &[ChplEntry]) -> Vec<u8> {
    if entries.is_empty() {
        return mp4;
    }

    let Some(moov_range) = find_box(&mp4, *b"moov") else {
        return mp4;
    };

    let chpl_bytes = build_chpl(entries);
    let moov_slice = &mp4[moov_range.clone()];

    if let Some(udta_rel) = find_child_box(moov_slice, *b"udta") {
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
            return mp4;
        }
        let mut new_udta = Vec::with_capacity(new_udta_size);
        new_udta.extend_from_slice(&(new_udta_size as u32).to_be_bytes());
        new_udta.extend_from_slice(b"udta");
        new_udta.extend_from_slice(&new_udta_body);
        new_udta.extend_from_slice(&chpl_bytes);

        let udta_delta = (new_udta.len() as i64) - (udta_abs.len() as i64);
        let new_moov_size = (moov_range.len() as i64) + udta_delta;
        if new_moov_size < 8 || new_moov_size > i64::from(u32::MAX) {
            return mp4;
        }

        let out_capacity = (mp4.len() as i64 + udta_delta) as usize;
        let mut out = Vec::with_capacity(out_capacity);
        out.extend_from_slice(&mp4[..moov_range.start]);
        out.extend_from_slice(&(new_moov_size as u32).to_be_bytes());
        out.extend_from_slice(b"moov");
        out.extend_from_slice(&mp4[moov_range.start + 8..udta_abs.start]);
        out.extend_from_slice(&new_udta);
        out.extend_from_slice(&mp4[udta_abs.end..moov_range.end]);
        out.extend_from_slice(&mp4[moov_range.end..]);
        out
    } else {
        let new_udta_size = 8 + chpl_bytes.len();
        if new_udta_size > u32::MAX as usize {
            return mp4;
        }
        let mut new_udta = Vec::with_capacity(new_udta_size);
        new_udta.extend_from_slice(&(new_udta_size as u32).to_be_bytes());
        new_udta.extend_from_slice(b"udta");
        new_udta.extend_from_slice(&chpl_bytes);

        let new_moov_size = moov_range.len() + new_udta_size;
        if new_moov_size > u32::MAX as usize {
            return mp4;
        }

        let mut out = Vec::with_capacity(mp4.len() + new_udta_size);
        out.extend_from_slice(&mp4[..moov_range.start]);
        out.extend_from_slice(&(new_moov_size as u32).to_be_bytes());
        out.extend_from_slice(b"moov");
        out.extend_from_slice(&mp4[moov_range.start + 8..moov_range.end]);
        out.extend_from_slice(&new_udta);
        out.extend_from_slice(&mp4[moov_range.end..]);
        out
    }
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
/// (potentially modified) bytes.  If there are no markers or the MP4 is
/// malformed, returns the bytes unchanged.
#[must_use]
pub fn embed_chapters(
    mp4_bytes: Vec<u8>,
    markers: &[Marker],
    range_start: Time,
    range_end: Time,
) -> Vec<u8> {
    match chapter_entries(markers, range_start, range_end) {
        Some(entries) => inject_chapters_into_mp4(mp4_bytes, &entries),
        None => mp4_bytes,
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
            ChplEntry { ts_100ns: 0, title: b"Intro".to_vec() },
            ChplEntry { ts_100ns: 10_000_000, title: b"Part 2".to_vec() },
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
        let mut entry = ChplEntry { ts_100ns: 0, title: long.into_bytes() };
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
            0, 0, 0, 16,
            b'f', b't', b'y', b'p',
            b'i', b's', b'o', b'm',
            0, 0, 2, 0,
        ];
        out.extend_from_slice(ftyp);

        let mvhd_body: &[u8] = &[
            0, 0, 0, 40,
            b'm', b'v', b'h', b'd',
            0, 0, 0, 0,
            0, 0, 0, 0,
            0, 0, 0, 0,
            0, 0, 3, 232,
            0, 0, 0, 0,
            0, 1, 0, 0,
            1, 0, 0, 0,
            0, 0, 0, 1,
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
        base[moov_range.start..moov_range.start + 4]
            .copy_from_slice(&new_moov_size.to_be_bytes());
        base.extend_from_slice(udta);
        base
    }

    #[test]
    fn inject_into_mp4_without_udta_creates_udta_with_chpl() {
        let mp4 = minimal_mp4_no_udta();
        let entry = ChplEntry { ts_100ns: 0, title: b"Start".to_vec() };
        let result = inject_chapters_into_mp4(mp4, &[entry]);

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
        let entry = ChplEntry { ts_100ns: 500_000, title: b"Mid".to_vec() };
        let result = inject_chapters_into_mp4(mp4, &[entry]);

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
        let first = vec![ChplEntry { ts_100ns: 0, title: b"Old".to_vec() }];
        let mp4_with_chpl = inject_chapters_into_mp4(mp4, &first);
        let second = vec![ChplEntry { ts_100ns: 0, title: b"New".to_vec() }];
        let result = inject_chapters_into_mp4(mp4_with_chpl, &second);

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
        let result = inject_chapters_into_mp4(mp4, &[]);
        assert_eq!(result.len(), orig_len);
    }

    #[test]
    fn not_valid_mp4_returns_original() {
        let garbage = b"not an mp4".to_vec();
        let entry = ChplEntry { ts_100ns: 0, title: b"X".to_vec() };
        let result = inject_chapters_into_mp4(garbage.clone(), &[entry]);
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
        let entries = entries_from_markers(&markers, Time(TICKS_PER_SECOND), Time(2 * TICKS_PER_SECOND));
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
}
