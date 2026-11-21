//! Embed timeline markers as a Nero `chpl` chapter list in a finished MP4.
//!
//! # Why a post-mux patch
//!
//! `filmcraft-isobmff`'s `Mp4Writer` only writes `©xxx` text items into `moov/udta`
//! (`WriterOptions::metadata`); it has no way to add an arbitrary user box. The encoder
//! also writes `moov` in front of `mdat` (faststart), so the box is added after muxing,
//! in the finished file, and everything that pointed behind `moov` is fixed up:
//!
//! 1. the new `moov` is built (`udta` created or extended, any older `chpl` replaced);
//! 2. every `stco`/`co64` entry that pointed at or after the end of the old `moov` moves
//!    by exactly the size change of `moov`. Entries in front of `moov` (the plain layout
//!    with `mdat` first) are left alone;
//! 3. the old `moov` bytes are replaced in place.
//!
//! A shifted `stco` entry that no longer fits 32 bits is an error. Converting the table to
//! `co64` would change `moov`'s size again and with it the shift; the muxer already picks
//! `co64` by itself when a file is large, so this only happens for a file within the size
//! of a `moov` edit of the 4 GiB boundary.
//!
//! # Why Nero `chpl`
//!
//! A `QuickTime` chapter track needs a full text track with sample data. `chpl` is one
//! small box that VLC, mpv and `FFmpeg` read. Layout (as `FFmpeg` writes and reads it): a
//! `FullBox` of version 1, four reserved bytes, a one-byte chapter count, then per
//! chapter a big-endian 64-bit start in 100 ns units and a length-prefixed UTF-8 title.
//! The one-byte count caps a file at [`MAX_CHAPTERS`] chapters; the first ones win.

use animathio_core::{Marker, TICKS_PER_SECOND, TimeRange};
use std::ops::Range;

/// Most chapters a `chpl` box can hold (its count is one byte).
pub const MAX_CHAPTERS: usize = 255;
/// Longest chapter title in UTF-8 bytes (its length is one byte).
const MAX_TITLE_BYTES: usize = 255;
/// 100-ns units per second.
const UNITS_PER_SECOND: u128 = 10_000_000;

/// Why chapters could not be embedded.
#[derive(Debug, thiserror::Error)]
pub enum ChapterError {
    /// The MP4 has no `moov` box.
    #[error("the MP4 has no moov box")]
    NoMoov,
    /// The box structure is inconsistent.
    #[error("malformed MP4: {0}")]
    Malformed(&'static str),
    /// Fragment boxes carry absolute offsets that this patch does not maintain.
    #[error("fragmented MP4 is not supported ({0} box present)")]
    Fragmented(String),
    /// A chunk offset cannot move by the `moov` growth within its table width.
    #[error("chunk offset {offset} cannot move by {delta} bytes within a {table} table")]
    OffsetOverflow {
        /// `stco` or `co64`.
        table: &'static str,
        /// The offset before the move.
        offset: u64,
        /// The signed change of `moov`'s size.
        delta: i64,
    },
}

/// A chapter as `chpl` stores it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Chapter {
    /// Start in 100-ns units, relative to the exported range.
    pub(crate) start: u64,
    /// UTF-8 title, at most [`MAX_TITLE_BYTES`] bytes.
    pub(crate) title: String,
}

/// The chapters for the markers inside `range`, relative to its start.
///
/// A marker belongs to the range when `start <= time < end`. An unnamed marker becomes
/// `Chapter N` where `N` is its 1-based position among *all* the markers ordered by time
/// (the numbering of the `chapters.export` sidecar, so a marker keeps its number whichever
/// range is exported and the first chapter of a trimmed export may be `Chapter 3`). Unlike
/// the sidecar, embedded chapters do not merge close markers and add no intro chapter. A
/// marker that lands on the same 100-ns instant as an earlier one is dropped, and the list
/// stops at [`MAX_CHAPTERS`].
pub(crate) fn chapters_from_markers(markers: &[Marker], range: TimeRange) -> Vec<Chapter> {
    let mut sorted: Vec<&Marker> = markers.iter().collect();
    sorted.sort_by_key(|m| (m.time, m.id));
    let mut out: Vec<Chapter> = Vec::new();
    for (index, marker) in sorted.into_iter().enumerate() {
        if marker.time < range.start || marker.time >= range.end {
            continue;
        }
        if out.len() == MAX_CHAPTERS {
            break;
        }
        let relative = u128::try_from(marker.time.0 - range.start.0).unwrap_or(0);
        let start = u64::try_from(relative * UNITS_PER_SECOND / TICKS_PER_SECOND as u128)
            .unwrap_or(u64::MAX);
        if out.last().is_some_and(|last| last.start == start) {
            continue;
        }
        let name = marker.name.split_whitespace().collect::<Vec<_>>().join(" ");
        let title = if name.is_empty() {
            format!("Chapter {}", index + 1)
        } else {
            truncate_utf8(name, MAX_TITLE_BYTES)
        };
        out.push(Chapter { start, title });
    }
    out
}

fn truncate_utf8(mut text: String, max: usize) -> String {
    if text.len() > max {
        let mut end = max;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    text
}

/// The complete `chpl` box (header included) for `chapters`.
fn build_chpl(chapters: &[Chapter]) -> Vec<u8> {
    let mut payload = vec![1, 0, 0, 0, 0, 0, 0, 0];
    payload.push(u8::try_from(chapters.len()).unwrap_or(u8::MAX));
    for chapter in chapters {
        payload.extend_from_slice(&chapter.start.to_be_bytes());
        let title = chapter.title.as_bytes();
        payload.push(u8::try_from(title.len()).unwrap_or(u8::MAX));
        payload.extend_from_slice(title);
    }
    let mut out = box_header(*b"chpl", payload.len(), false);
    out.extend_from_slice(&payload);
    out
}

/// A box header for a payload of `payload_len` bytes. The 64-bit form is used when asked
/// for (the box had it) or when the box does not fit 32 bits.
fn box_header(kind: [u8; 4], payload_len: usize, wide: bool) -> Vec<u8> {
    let small = u32::try_from(payload_len + 8).ok().filter(|_| !wide);
    let mut out = Vec::with_capacity(16);
    if let Some(size) = small {
        out.extend_from_slice(&size.to_be_bytes());
        out.extend_from_slice(&kind);
    } else {
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(&kind);
        out.extend_from_slice(&((payload_len + 16) as u64).to_be_bytes());
    }
    out
}

/// One box located in a byte slice.
#[derive(Clone, Copy, Debug)]
struct BoxRef {
    kind: [u8; 4],
    /// Offset of the first header byte.
    start: usize,
    /// Header length: 8, or 16 with a 64-bit size.
    header: usize,
    /// Offset one past the last byte.
    end: usize,
}

impl BoxRef {
    fn payload(&self) -> Range<usize> {
        self.start + self.header..self.end
    }
}

/// The boxes laid end to end in `data[range]`. A size of 1 means a 64-bit size follows
/// the type and a size of 0 means the box runs to the end of `range`.
fn read_boxes(data: &[u8], range: Range<usize>) -> Result<Vec<BoxRef>, ChapterError> {
    let mut out = Vec::new();
    let mut pos = range.start;
    while pos < range.end {
        if range.end - pos < 8 {
            return Err(ChapterError::Malformed("truncated box header"));
        }
        let size32 = u32::from_be_bytes(data[pos..pos + 4].try_into().expect("4 bytes"));
        let kind: [u8; 4] = data[pos + 4..pos + 8].try_into().expect("4 bytes");
        let (header, size) = match size32 {
            0 => (8, range.end - pos),
            1 => {
                if range.end - pos < 16 {
                    return Err(ChapterError::Malformed("truncated 64-bit box size"));
                }
                let large = u64::from_be_bytes(data[pos + 8..pos + 16].try_into().expect("8"));
                let size = usize::try_from(large)
                    .map_err(|_| ChapterError::Malformed("box size exceeds memory"))?;
                (16, size)
            }
            n => (8, n as usize),
        };
        if size < header || size > range.end - pos {
            return Err(ChapterError::Malformed("box size outside its parent"));
        }
        out.push(BoxRef {
            kind,
            start: pos,
            header,
            end: pos + size,
        });
        pos += size;
    }
    Ok(out)
}

/// Embed `markers` inside `range` as chapters. Returns `mp4` unchanged when no marker is
/// inside the range.
///
/// # Errors
///
/// When the MP4 has no `moov`, is malformed or fragmented, or a chunk offset cannot be
/// shifted (see the module documentation).
pub fn embed_chapters(
    mp4: Vec<u8>,
    markers: &[Marker],
    range: TimeRange,
) -> Result<Vec<u8>, ChapterError> {
    let chapters = chapters_from_markers(markers, range);
    if chapters.is_empty() {
        return Ok(mp4);
    }
    inject_chpl(mp4, &build_chpl(&chapters))
}

/// Put `chpl` (a complete box) into `moov/udta` and repair the chunk offsets.
fn inject_chpl(mut mp4: Vec<u8>, chpl: &[u8]) -> Result<Vec<u8>, ChapterError> {
    let top = read_boxes(&mp4, 0..mp4.len())?;
    if let Some(fragment) = top
        .iter()
        .find(|b| matches!(&b.kind, b"moof" | b"sidx" | b"mfra" | b"ssix"))
    {
        return Err(ChapterError::Fragmented(
            String::from_utf8_lossy(&fragment.kind).into_owned(),
        ));
    }
    let moov = *top
        .iter()
        .find(|b| &b.kind == b"moov")
        .ok_or(ChapterError::NoMoov)?;

    // New moov payload: every child verbatim, except that udta gains the chpl.
    let mut payload = Vec::with_capacity(moov.end - moov.start + chpl.len());
    let mut had_udta = false;
    for child in read_boxes(&mp4, moov.payload())? {
        if &child.kind == b"udta" && !had_udta {
            had_udta = true;
            let mut udta = Vec::new();
            for entry in read_udta(&mp4, &child)? {
                if &entry.kind != b"chpl" {
                    udta.extend_from_slice(&mp4[entry.start..entry.end]);
                }
            }
            udta.extend_from_slice(chpl);
            payload.extend(box_header(*b"udta", udta.len(), child.header == 16));
            payload.extend(udta);
        } else {
            payload.extend_from_slice(&mp4[child.start..child.end]);
        }
    }
    if !had_udta {
        payload.extend(box_header(*b"udta", chpl.len(), false));
        payload.extend_from_slice(chpl);
    }
    let mut new_moov = box_header(*b"moov", payload.len(), moov.header == 16);
    new_moov.extend(payload);

    let delta = i64::try_from(new_moov.len())
        .and_then(|new| i64::try_from(moov.end - moov.start).map(|old| new - old))
        .map_err(|_| ChapterError::Malformed("moov exceeds the supported size"))?;
    shift_chunk_offsets(&mut new_moov, moov.end as u64, delta)?;
    mp4.splice(moov.start..moov.end, new_moov);
    Ok(mp4)
}

/// The boxes inside a `udta`. `QuickTime` may end the box with four zero bytes; those
/// are dropped.
fn read_udta(mp4: &[u8], udta: &BoxRef) -> Result<Vec<BoxRef>, ChapterError> {
    let mut range = udta.payload();
    let mut pos = range.start;
    // Walk to find a four-byte terminator without failing on it.
    while range.end - pos >= 8 {
        let size = u32::from_be_bytes(mp4[pos..pos + 4].try_into().expect("4 bytes")) as usize;
        if size == 0 || size == 1 {
            break;
        }
        pos += size.min(range.end - pos);
    }
    if pos < range.end && range.end - pos == 4 && mp4[pos..range.end] == [0; 4] {
        range.end = pos;
    }
    read_boxes(mp4, range)
}

/// Move every `stco`/`co64` entry that points at or after `old_moov_end` by `delta`. The
/// offsets in `moov` are absolute positions in the file before the edit.
fn shift_chunk_offsets(moov: &mut [u8], old_moov_end: u64, delta: i64) -> Result<(), ChapterError> {
    let root = read_boxes(moov, 0..moov.len())?[0];
    let mut tables = Vec::new();
    collect_tables(moov, &root, 0, &mut tables)?;
    for (kind, payload) in tables {
        let (table, width) = if &kind == b"stco" {
            ("stco", 4)
        } else {
            ("co64", 8)
        };
        if payload.len() < 8 {
            return Err(ChapterError::Malformed("short chunk offset table"));
        }
        let count = u32::from_be_bytes(
            moov[payload.start + 4..payload.start + 8]
                .try_into()
                .expect("4"),
        ) as usize;
        let entries = payload.start + 8;
        if count
            .checked_mul(width)
            .and_then(|n| n.checked_add(entries))
            .is_none_or(|end| end > payload.end)
        {
            return Err(ChapterError::Malformed(
                "chunk offset table overruns its box",
            ));
        }
        for i in 0..count {
            let at = entries + i * width;
            let offset = if width == 4 {
                u64::from(u32::from_be_bytes(moov[at..at + 4].try_into().expect("4")))
            } else {
                u64::from_be_bytes(moov[at..at + 8].try_into().expect("8"))
            };
            if offset < old_moov_end {
                continue;
            }
            let overflow = ChapterError::OffsetOverflow {
                table,
                offset,
                delta,
            };
            let moved = offset.checked_add_signed(delta).ok_or(overflow)?;
            if width == 4 {
                let moved = u32::try_from(moved).map_err(|_| ChapterError::OffsetOverflow {
                    table,
                    offset,
                    delta,
                })?;
                moov[at..at + 4].copy_from_slice(&moved.to_be_bytes());
            } else {
                moov[at..at + 8].copy_from_slice(&moved.to_be_bytes());
            }
        }
    }
    Ok(())
}

/// Collect the payload ranges of every `stco`/`co64` under `moov/trak/mdia/minf/stbl`.
fn collect_tables(
    data: &[u8],
    parent: &BoxRef,
    depth: usize,
    out: &mut Vec<([u8; 4], Range<usize>)>,
) -> Result<(), ChapterError> {
    const PATH: [&[u8; 4]; 4] = [b"trak", b"mdia", b"minf", b"stbl"];
    for child in read_boxes(data, parent.payload())? {
        if depth == PATH.len() {
            if matches!(&child.kind, b"stco" | b"co64") {
                out.push((child.kind, child.payload()));
            }
        } else if &child.kind == PATH[depth] {
            collect_tables(data, &child, depth + 1, out)?;
        }
    }
    Ok(())
}

/// Parse the `chpl` out of a finished MP4: `(start in 100 ns, title)` per chapter.
#[cfg(test)]
pub(crate) fn read_chpl(mp4: &[u8]) -> Option<Vec<(u64, String)>> {
    let moov = read_boxes(mp4, 0..mp4.len())
        .ok()?
        .into_iter()
        .find(|b| &b.kind == b"moov")?;
    let udta = read_boxes(mp4, moov.payload())
        .ok()?
        .into_iter()
        .find(|b| &b.kind == b"udta")?;
    let chpl = read_udta(mp4, &udta)
        .ok()?
        .into_iter()
        .find(|b| &b.kind == b"chpl")?;
    let body = &mp4[chpl.payload()];
    // version 1, flags, four reserved bytes, count.
    assert_eq!(body[0], 1, "chpl version");
    let count = usize::from(body[8]);
    let mut pos = 9;
    let mut out = Vec::new();
    for _ in 0..count {
        let start = u64::from_be_bytes(body[pos..pos + 8].try_into().ok()?);
        let len = usize::from(body[pos + 8]);
        let title = String::from_utf8(body[pos + 9..pos + 9 + len].to_vec()).ok()?;
        pos += 9 + len;
        out.push((start, title));
    }
    assert_eq!(pos, body.len(), "chpl has trailing bytes");
    Some(out)
}

/// The type of every top-level box of a finished MP4, in file order.
#[cfg(test)]
pub(crate) fn top_level_kinds(mp4: &[u8]) -> Vec<[u8; 4]> {
    read_boxes(mp4, 0..mp4.len())
        .expect("well-formed boxes")
        .into_iter()
        .map(|b| b.kind)
        .collect()
}

#[cfg(test)]
mod tests {
    // Test fixtures build byte layouts with small, known values.
    #![allow(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_precision_loss,
        clippy::trivially_copy_pass_by_ref
    )]

    use super::*;
    use animathio_core::{MarkerId, Time};

    fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = box_header(*kind, payload.len(), false);
        out.extend_from_slice(payload);
        out
    }

    fn bx64(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = box_header(*kind, payload.len(), true);
        out.extend_from_slice(payload);
        out
    }

    fn table(kind: &[u8; 4], offsets: &[u64]) -> Vec<u8> {
        let mut payload = vec![0; 4];
        payload.extend((offsets.len() as u32).to_be_bytes());
        for &o in offsets {
            if kind == b"stco" {
                payload.extend((o as u32).to_be_bytes());
            } else {
                payload.extend(o.to_be_bytes());
            }
        }
        bx(kind, &payload)
    }

    /// `trak/mdia/minf/stbl/<table>`.
    fn trak(table: &[u8]) -> Vec<u8> {
        bx(b"trak", &bx(b"mdia", &bx(b"minf", &bx(b"stbl", table))))
    }

    /// Chunk offsets of every table in a file, in file order.
    fn offsets(mp4: &[u8]) -> Vec<u64> {
        let moov = read_boxes(mp4, 0..mp4.len())
            .unwrap()
            .into_iter()
            .find(|b| &b.kind == b"moov")
            .unwrap();
        let mut tables = Vec::new();
        collect_tables(mp4, &moov, 0, &mut tables).unwrap();
        let mut out = Vec::new();
        for (kind, payload) in tables {
            let count = u32::from_be_bytes(
                mp4[payload.start + 4..payload.start + 8]
                    .try_into()
                    .unwrap(),
            );
            for i in 0..count as usize {
                if &kind == b"stco" {
                    let at = payload.start + 8 + i * 4;
                    out.push(u64::from(u32::from_be_bytes(
                        mp4[at..at + 4].try_into().unwrap(),
                    )));
                } else {
                    let at = payload.start + 8 + i * 8;
                    out.push(u64::from_be_bytes(mp4[at..at + 8].try_into().unwrap()));
                }
            }
        }
        out
    }

    fn one_chapter() -> Vec<u8> {
        build_chpl(&[Chapter {
            start: 0,
            title: "A".into(),
        }])
    }

    /// `ftyp`, then `moov` (stco + co64 tracks), then `mdat`. Offsets point into mdat.
    fn faststart_file(moov_extra: &[u8]) -> (Vec<u8>, Vec<u64>) {
        let ftyp = bx(b"ftyp", b"isom\0\0\0\0isom");
        let moov_len = |stco: &[u64], co64: &[u64]| {
            bx(
                b"moov",
                &[
                    trak(&table(b"stco", stco)),
                    trak(&table(b"co64", co64)),
                    moov_extra.to_vec(),
                ]
                .concat(),
            )
            .len()
        };
        let moov_size = moov_len(&[0, 0], &[0, 0]);
        let mdat_start = (ftyp.len() + moov_size + 8) as u64;
        let stco = [mdat_start, mdat_start + 100];
        let co64 = [mdat_start + 40, mdat_start + 4000];
        let moov = bx(
            b"moov",
            &[
                trak(&table(b"stco", &stco)),
                trak(&table(b"co64", &co64)),
                moov_extra.to_vec(),
            ]
            .concat(),
        );
        assert_eq!(moov.len(), moov_size);
        let file = [ftyp, moov, bx(b"mdat", &vec![7; 5000])].concat();
        (file, [stco, co64].concat())
    }

    #[test]
    fn faststart_offsets_shift_by_exactly_the_moov_growth() {
        let (file, before) = faststart_file(&[]);
        let moov_before = file.len();
        let chpl = one_chapter();
        let patched = inject_chpl(file.clone(), &chpl).unwrap();
        // No udta before: udta header + chpl are new.
        let growth = (8 + chpl.len()) as u64;
        assert_eq!(patched.len() as u64, moov_before as u64 + growth);
        let after = offsets(&patched);
        assert_eq!(after.len(), before.len());
        for (b, a) in before.iter().zip(&after) {
            assert_eq!(*a, b + growth);
        }
        // The shifted offsets still point at the same mdat bytes: the first stco entry is
        // the first mdat payload byte.
        assert_eq!(patched[after[0] as usize], 7);
        assert_eq!(read_chpl(&patched).unwrap(), vec![(0, "A".to_string())]);
    }

    #[test]
    fn replacing_a_larger_chpl_shrinks_and_shifts_back() {
        let big = build_chpl(&[Chapter {
            start: 5,
            title: "a long title that makes the first chpl big".into(),
        }]);
        let udta = bx(b"udta", &big);
        let (file, before) = faststart_file(&udta);
        let patched = inject_chpl(file.clone(), &one_chapter()).unwrap();
        let after = offsets(&patched);
        let shrink = (big.len() - one_chapter().len()) as u64;
        assert_eq!(patched.len() as u64 + shrink, file.len() as u64);
        for (b, a) in before.iter().zip(&after) {
            assert_eq!(*a + shrink, *b);
        }
        assert_eq!(read_chpl(&patched).unwrap().len(), 1);
    }

    #[test]
    fn non_faststart_offsets_are_untouched() {
        // ftyp, mdat, moov: every offset is behind the old moov start, i.e. before it.
        let ftyp = bx(b"ftyp", b"isom\0\0\0\0isom");
        let mdat = bx(b"mdat", &vec![7; 5000]);
        let first = (ftyp.len() + 8) as u64;
        let stco = [first, first + 100];
        let co64 = [first + 40, first + 4000];
        let moov = bx(
            b"moov",
            &[trak(&table(b"stco", &stco)), trak(&table(b"co64", &co64))].concat(),
        );
        let file = [ftyp, mdat, moov].concat();
        let patched = inject_chpl(file.clone(), &one_chapter()).unwrap();
        assert!(patched.len() > file.len());
        assert_eq!(offsets(&patched), offsets(&file));
        assert_eq!(patched[offsets(&patched)[0] as usize], 7);
        assert_eq!(read_chpl(&patched).unwrap().len(), 1);
    }

    #[test]
    fn stco_overflow_is_an_error_not_a_conversion() {
        let ftyp = bx(b"ftyp", b"isom\0\0\0\0isom");
        let moov = bx(b"moov", &trak(&table(b"stco", &[u64::from(u32::MAX) - 4])));
        let file = [ftyp, moov].concat();
        let err = inject_chpl(file, &one_chapter()).unwrap_err();
        assert!(
            matches!(err, ChapterError::OffsetOverflow { table: "stco", .. }),
            "{err}"
        );
    }

    #[test]
    fn largesize_and_to_end_boxes_are_walked() {
        let ftyp = bx64(b"ftyp", b"isom\0\0\0\0isom");
        // moov with a 64-bit size, holding a 64-bit trak and a udta with 64-bit size.
        let udta = bx64(b"udta", &bx(b"free", &[1, 2, 3]));
        let stco = table(b"stco", &[0]);
        let trak = bx64(
            b"trak",
            &bx64(b"mdia", &bx64(b"minf", &bx64(b"stbl", &stco))),
        );
        let moov = bx64(b"moov", &[trak, udta].concat());
        // mdat with size 0: runs to the end of the file.
        let mut mdat = 0u32.to_be_bytes().to_vec();
        mdat.extend(b"mdat");
        mdat.extend(vec![9; 64]);
        let first = (ftyp.len() + moov.len() + 8) as u64;
        let mut file = [ftyp, moov, mdat].concat();
        // Point the single stco entry at mdat's first payload byte.
        let top = read_boxes(&file, 0..file.len()).unwrap();
        let mut tables = Vec::new();
        collect_tables(
            &file,
            top.iter().find(|b| &b.kind == b"moov").unwrap(),
            0,
            &mut tables,
        )
        .unwrap();
        let entry = tables[0].1.start + 8;
        file[entry..entry + 4].copy_from_slice(&(first as u32).to_be_bytes());
        assert_eq!(file[first as usize], 9);

        let patched = inject_chpl(file.clone(), &one_chapter()).unwrap();
        let after = offsets(&patched);
        assert_eq!(patched[after[0] as usize], 9);
        assert_eq!(read_chpl(&patched).unwrap(), vec![(0, "A".to_string())]);
        // The 64-bit headers were kept: moov and udta are still `size == 1` boxes.
        let top = read_boxes(&patched, 0..patched.len()).unwrap();
        let moov = top.iter().find(|b| &b.kind == b"moov").unwrap();
        assert_eq!(moov.header, 16);
        let udta = read_boxes(&patched, moov.payload())
            .unwrap()
            .into_iter()
            .find(|b| &b.kind == b"udta")
            .unwrap();
        assert_eq!(udta.header, 16);
        // The to-end mdat is still the last box and untouched.
        assert!(patched.ends_with(&[9; 64]));
    }

    #[test]
    fn malformed_and_missing_moov_are_errors() {
        assert!(matches!(
            inject_chpl(bx(b"ftyp", b"isom"), &one_chapter()),
            Err(ChapterError::NoMoov)
        ));
        let mut bad = bx(b"moov", &[0; 4]);
        bad[3] = 200;
        assert!(matches!(
            inject_chpl(bad, &one_chapter()),
            Err(ChapterError::Malformed(_))
        ));
        let fragmented = [bx(b"moov", &[]), bx(b"moof", &[])].concat();
        assert!(matches!(
            inject_chpl(fragmented, &one_chapter()),
            Err(ChapterError::Fragmented(_))
        ));
    }

    fn marker(id: u64, seconds: f64, name: &str) -> Marker {
        let mut m = Marker::new(
            MarkerId(id),
            Time((seconds * TICKS_PER_SECOND as f64) as i64),
        );
        m.name = name.into();
        m
    }

    #[test]
    fn chapters_follow_the_range_and_the_sidecar_naming() {
        let range =
            TimeRange::new(Time(2 * TICKS_PER_SECOND), Time(10 * TICKS_PER_SECOND)).unwrap();
        let markers = [
            marker(1, 1.0, "before"),
            marker(2, 2.0, "  Intro   part "),
            marker(3, 4.5, ""),
            marker(4, 4.5, "duplicate instant"),
            marker(5, 10.0, "at the end"),
            marker(6, 7.0, "Outro"),
        ];
        let chapters = chapters_from_markers(&markers, range);
        // `Chapter N` counts every marker by time: "before" is 1, "Intro part" 2, the unnamed
        // one 3.
        assert_eq!(
            chapters
                .iter()
                .map(|c| (c.start, c.title.as_str()))
                .collect::<Vec<_>>(),
            [
                (0, "Intro part"),
                (25_000_000, "Chapter 3"),
                (50_000_000, "Outro"),
            ]
        );
    }

    #[test]
    fn other_udta_items_survive_inserting_and_replacing_the_chpl() {
        let tool = bx(b"\xa9too", b"\0\x0e\0\0Lavf-test-item");
        let name = bx(b"\xa9nam", b"\0\x05\0\0title");
        let old_chpl = build_chpl(&[Chapter {
            start: 7,
            title: "an older, longer chapter title".into(),
        }]);
        // Insert: the udta has no chpl yet. Replace: it has one, between the other items.
        for udta_items in [
            [tool.clone(), name.clone()].concat(),
            [tool.clone(), old_chpl, name.clone()].concat(),
        ] {
            let (file, before) = faststart_file(&bx(b"udta", &udta_items));
            let patched = inject_chpl(file.clone(), &one_chapter()).unwrap();

            // Top level: ftyp, moov, mdat, with sizes that tile the file exactly.
            assert_eq!(
                top_level_kinds(&patched),
                [*b"ftyp", *b"moov", *b"mdat"].to_vec()
            );
            let top = read_boxes(&patched, 0..patched.len()).unwrap();
            let moov = top[1];
            // udta is the last moov child; its size covers exactly its children.
            let udta = *read_boxes(&patched, moov.payload())
                .unwrap()
                .last()
                .unwrap();
            assert_eq!(&udta.kind, b"udta");
            assert_eq!(udta.end, moov.end, "udta ends where moov ends");
            let items = read_boxes(&patched, udta.payload()).unwrap();
            let kinds: Vec<_> = items.iter().map(|b| b.kind).collect();
            assert_eq!(
                kinds,
                [*b"\xa9too", *b"\xa9nam", *b"chpl"].to_vec(),
                "the other items are kept in order and the chpl is last"
            );
            assert_eq!(&patched[items[0].start..items[0].end], tool.as_slice());
            assert_eq!(&patched[items[1].start..items[1].end], name.as_slice());
            assert_eq!(read_chpl(&patched).unwrap(), vec![(0, "A".to_string())]);

            // The chunk offsets moved by exactly the change of the moov size and still hit mdat.
            let old_moov = read_boxes(&file, 0..file.len()).unwrap()[1];
            let delta = (moov.end - moov.start) as i64 - (old_moov.end - old_moov.start) as i64;
            assert_eq!(patched.len() as i64, file.len() as i64 + delta);
            let after = offsets(&patched);
            assert_eq!(after.len(), before.len());
            for (b, a) in before.iter().zip(&after) {
                assert_eq!(*a as i64, *b as i64 + delta);
            }
            assert_eq!(patched[after[0] as usize], 7);
        }
    }

    #[test]
    fn chapter_count_and_title_length_are_capped_on_char_boundaries() {
        let range = TimeRange::new(Time(0), Time(1000 * TICKS_PER_SECOND)).unwrap();
        let markers: Vec<Marker> = (0..300)
            .map(|i| marker(i + 1, i as f64, &"é".repeat(200)))
            .collect();
        let chapters = chapters_from_markers(&markers, range);
        assert_eq!(chapters.len(), MAX_CHAPTERS);
        assert!(
            chapters
                .iter()
                .all(|c| c.title.len() <= 255 && c.title.chars().all(|ch| ch == 'é'))
        );
        assert_eq!(chapters[0].title.len(), 254);
    }

    #[test]
    fn no_markers_in_range_leaves_the_file_untouched() {
        let (file, _) = faststart_file(&[]);
        let range = TimeRange::new(Time(0), Time(TICKS_PER_SECOND)).unwrap();
        let out = embed_chapters(file.clone(), &[marker(1, 5.0, "late")], range).unwrap();
        assert_eq!(out, file);
    }
}
