//! Guard rail: the editor chrome takes its colours, sizes and radii from the
//! design tokens (`style/`), never from ad-hoc literals.
//!
//! Mirrors `glyph_guard.rs`: a small textual scanner over
//! `apps/animathio/src/**/*.rs` (outside the `style*` token modules) counting
//! raw colour constructors (`rgb(0x`, `rgba(0x`, `hsla(`), ad-hoc alpha tints
//! (`hexa(token, <number literal>)`), pixel-literal radii
//! (`.rounded(px(<number>`), raw rem lengths (`rems(<number>)`) and
//! `FontWeight::BOLD`. A file may never carry **more** hits than its
//! `BASELINE` entry; a surface PR (UI rework, PRs 2-6) lowers its files to 0
//! and removes their entries (see `docs/design.md`, the token sections).

/// Per-file counts recorded when the tokens landed (UI-rework PR 1). Files
/// missing from the table must have zero hits. Paths are relative to
/// `apps/animathio/src`.
#[cfg(test)]
const BASELINE: &[(&str, usize)] = &[
    ("editor/assist.rs", 1),
    ("editor/audio_effects.rs", 1),
    ("editor/community_plugins.rs", 2),
    ("editor/dialogs.rs", 3),
    ("editor/export_panel.rs", 1),
    ("editor/fields.rs", 11),
    ("editor/inspector_units.rs", 4),
    ("editor/inspector_view.rs", 2),
    ("editor/markers.rs", 3),
    ("editor/math.rs", 2),
    ("editor/media_pool.rs", 4),
    ("editor/menus.rs", 6),
    ("editor/recording.rs", 2),
    ("editor/reframe.rs", 1),
    ("editor/scenes.rs", 1),
    ("editor/start_screen.rs", 31),
    ("editor/subtitles.rs", 1),
    ("editor/templates.rs", 1),
    ("editor/titles.rs", 1),
    ("editor/video_effects.rs", 1),
    ("editor/workspace.rs", 4),
];

/// The number of ad-hoc literals in a source file (see the module docs). All
/// patterns count together.
#[cfg(test)]
fn hits(source: &str) -> usize {
    let mut count = 0;
    count += source.matches("rgb(0x").count();
    count += source.matches("rgba(0x").count();
    count += source.matches("hsla(").count();
    count += literal_alpha_hexa(source);
    count += followed_by_digit(source, ".rounded(px(");
    count += followed_by_digit(source, "rems(");
    count += source.matches("FontWeight::BOLD").count();
    count
}

/// `hexa(` calls whose alpha (the text after the last comma before the closing
/// paren) is a numeric literal, not a named token helper (`tint`) or a
/// variable.
#[cfg(test)]
fn literal_alpha_hexa(source: &str) -> usize {
    let bytes = source.as_bytes();
    let mut count = 0;
    let mut at = 0;
    while let Some(found) = source[at..].find("hexa(") {
        let start = at + found + "hexa(".len();
        let (args, close) = paren_body(bytes, start);
        at = close.max(start);
        if let Some(comma) = args.rfind(',') {
            let alpha = args[comma + 1..].trim();
            if alpha
                .chars()
                .next()
                .is_some_and(|first| first.is_ascii_digit() || first == '-')
            {
                count += 1;
            }
        }
    }
    count
}

/// The text between `start` and its matching close paren (nesting-aware), plus
/// the byte offset just past that paren.
#[cfg(test)]
fn paren_body(bytes: &[u8], start: usize) -> (&str, usize) {
    let mut depth = 1usize;
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return (std::str::from_utf8(&bytes[start..i]).unwrap_or(""), i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    (
        std::str::from_utf8(&bytes[start..]).unwrap_or(""),
        bytes.len(),
    )
}

/// Occurrences of `marker` immediately followed by (optional whitespace then)
/// an ASCII digit.
#[cfg(test)]
fn followed_by_digit(source: &str, marker: &str) -> usize {
    let mut count = 0;
    let mut at = 0;
    while let Some(found) = source[at..].find(marker) {
        let after = at + found + marker.len();
        if source[after..]
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_digit())
        {
            count += 1;
        }
        at = after.max(at + 1);
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_only_the_targeted_patterns() {
        let clean = "let a = hex(color::ACCENT); let b = tint(color::BG, Alpha::Scrim);";
        assert_eq!(hits(clean), 0);
        let dirty = "rgb(0x123456) hsla(0.5, 1., 1., 1.) hexa(color::ACCENT, 0.55) \
                     hexa(color::ACCENT, alpha) hexa(colors::WARNING, 0.3) \
                     .rounded(px(6.)) .rounded(px(radius::PANEL)) rems(1.5) \
                     rems(row) FontWeight::BOLD";
        assert_eq!(hits(dirty), 7);
    }

    #[test]
    fn hexa_with_nested_args_is_parsed() {
        let nested = "hexa(rgb(0x112233), 0.2)";
        assert_eq!(literal_alpha_hexa(nested), 1);
        let named = "hexa(color::ACCENT, Alpha::Wash.value())";
        assert_eq!(literal_alpha_hexa(named), 0);
    }

    #[test]
    fn no_file_exceeds_its_baseline() {
        let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        collect_rs(&src_dir, &mut files);
        assert!(
            files.len() > 5,
            "scanner found no sources under {src_dir:?}"
        );
        let mut offenders: Vec<String> = Vec::new();
        for (path, allowed) in BASELINE {
            let file = src_dir.join(path);
            let Ok(source) = std::fs::read_to_string(&file) else {
                offenders.push(format!("{path}: baseline entry but the file is missing"));
                continue;
            };
            let count = hits(&source);
            if count > *allowed {
                offenders.push(format!("{path}: {count} hits, baseline allows {allowed}"));
            }
        }
        for file in files {
            let relative = file
                .strip_prefix(&src_dir)
                .expect("walked files live under src")
                .to_string_lossy()
                .replace('\\', "/");
            // Token modules (style/) define the values; style_guard.rs (this
            // file) and glyph_guard.rs hold the patterns as text. `style*`
            // keeps the guard and the tokens out (the plan's carve-out).
            if relative.starts_with("style") {
                continue;
            }
            let allowed = BASELINE
                .iter()
                .find(|(path, _)| *path == relative)
                .map_or(0, |(_, allowed)| *allowed);
            let Ok(source) = std::fs::read_to_string(&file) else {
                continue;
            };
            let count = hits(&source);
            if count > allowed {
                offenders.push(format!(
                    "{relative}: {count} ad-hoc colour/size literals (baseline {allowed}); \
                     take the values from apps/animathio/src/style (see docs/design.md)"
                ));
            }
        }
        assert!(
            offenders.is_empty(),
            "new ad-hoc colour/size literals are not allowed (docs/design.md):\n{}",
            offenders.join("\n")
        );
    }

    #[test]
    fn baseline_entries_are_not_stale() {
        // A row for a file that reached zero hits is stale: the surface PR that
        // cleaned the file removes its row, so the file is guarded at zero.
        let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        for (path, allowed) in BASELINE {
            let source = std::fs::read_to_string(src_dir.join(path)).expect("baseline file");
            assert!(
                hits(&source) > 0 && *allowed > 0,
                "{path}: no ad-hoc literals left; remove its row from BASELINE"
            );
        }
    }

    fn collect_rs(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_rs(&path, files);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
        files.sort();
    }
}
