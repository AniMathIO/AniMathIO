//! Guard rail: the editor never uses Unicode symbol glyphs as icons.
//!
//! Pictograms are Lucide SVGs via `gpui-component`'s `Icon`, so this module only
//! exists to prove, in a unit test, that no string or character literal in
//! `apps/animathio/src/**/*.rs` contains a character from the emoji/symbol
//! ranges that used to stand in for icons (play/pause arrows, diamonds, warning
//! signs, bullets, minus signs, ...). The scanner is a small Rust lexer, so
//! comments, lifetimes and raw strings are handled and `\u{...}` escapes are
//! decoded before checking.

/// Exact literal contents that may contain symbol characters because they are
/// maths text in sample content rather than icons. Keep this list empty unless a
/// genuine maths string needs it.
#[cfg(test)]
const MATH_TEXT_ALLOWLIST: &[&str] = &[];

/// Characters that must not appear as UI icons in literals.
#[cfg(test)]
fn forbidden(c: char) -> bool {
    let code = u32::from(c);
    (0x2190..=0x2BFF).contains(&code) // arrows, math operators, misc symbols, blocks
        || (0x1F000..=0x1FAFF).contains(&code) // emoji & pictographs
        || (0x2600..=0x27BF).contains(&code) // misc symbols & dingbats
        || (0x25A0..=0x25FF).contains(&code) // geometric shapes
}

/// A decoded literal and the byte offset where it starts.
#[cfg(test)]
struct Literal {
    offset: usize,
    text: String,
}

/// Extract the decoded contents of every string, byte-string, raw-string and
/// character literal, skipping comments and lifetimes.
#[cfg(test)]
fn literals(source: &str) -> Vec<Literal> {
    let chars: Vec<(usize, char)> = source.char_indices().collect();
    let at = |i: usize| chars.get(i).map(|(_, c)| *c);
    let mut found = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let (offset, c) = chars[i];
        match c {
            '/' if at(i + 1) == Some('/') => {
                while i < chars.len() && chars[i].1 != '\n' {
                    i += 1;
                }
            }
            '/' if at(i + 1) == Some('*') => {
                // Block comments nest in Rust.
                let mut depth = 1;
                i += 2;
                while i < chars.len() && depth > 0 {
                    if at(i) == Some('/') && at(i + 1) == Some('*') {
                        depth += 1;
                        i += 2;
                    } else if at(i) == Some('*') && at(i + 1) == Some('/') {
                        depth -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
            }
            'r' | 'b' if is_raw_string_start(&chars, i) => {
                let mut j = i + 1;
                if at(j) == Some('r') {
                    j += 1;
                }
                let mut hashes = 0;
                while at(j) == Some('#') {
                    hashes += 1;
                    j += 1;
                }
                j += 1; // opening quote
                let mut text = String::new();
                while j < chars.len() {
                    if at(j) == Some('"') && (1..=hashes).all(|k| at(j + k) == Some('#')) {
                        j += 1 + hashes;
                        break;
                    }
                    text.push(chars[j].1);
                    j += 1;
                }
                found.push(Literal { offset, text });
                i = j;
            }
            '"' => {
                let (text, next) = read_quoted(&chars, i + 1, '"');
                found.push(Literal { offset, text });
                i = next;
            }
            'b' if at(i + 1) == Some('"') => {
                let (text, next) = read_quoted(&chars, i + 2, '"');
                found.push(Literal { offset, text });
                i = next;
            }
            '\'' => {
                // A character literal is `'x'` or `'\...'`; anything else is a
                // lifetime or label and carries no text.
                let is_char = at(i + 1) == Some('\\') || at(i + 2) == Some('\'');
                if is_char {
                    let (text, next) = read_quoted(&chars, i + 1, '\'');
                    found.push(Literal { offset, text });
                    i = next;
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    found
}

/// Whether `r"`, `r#"`, `br"` or `br#"` starts at `i` (and is not part of an
/// identifier such as `bar"`).
#[cfg(test)]
fn is_raw_string_start(chars: &[(usize, char)], i: usize) -> bool {
    let at = |k: usize| chars.get(k).map(|(_, c)| *c);
    if i > 0 && at(i - 1).is_some_and(|c| c.is_alphanumeric() || c == '_') {
        return false;
    }
    let mut j = i;
    if at(j) == Some('b') {
        j += 1;
    }
    if at(j) != Some('r') {
        return false;
    }
    j += 1;
    while at(j) == Some('#') {
        j += 1;
    }
    at(j) == Some('"')
}

/// Read up to the closing `quote`, decoding the escapes that can spell a symbol
/// (`\u{...}`); other escapes are dropped.
#[cfg(test)]
fn read_quoted(chars: &[(usize, char)], start: usize, quote: char) -> (String, usize) {
    let mut content = String::new();
    let mut j = start;
    while j < chars.len() {
        let c = chars[j].1;
        if c == '\\' {
            if chars.get(j + 1).map(|(_, c)| *c) == Some('u')
                && chars.get(j + 2).map(|(_, c)| *c) == Some('{')
            {
                let mut k = j + 3;
                let mut hex = String::new();
                while k < chars.len() && chars[k].1 != '}' {
                    hex.push(chars[k].1);
                    k += 1;
                }
                if let Some(decoded) = u32::from_str_radix(&hex.replace('_', ""), 16)
                    .ok()
                    .and_then(char::from_u32)
                {
                    content.push(decoded);
                }
                j = k + 1;
            } else {
                j += 2;
            }
        } else if c == quote {
            return (content, j + 1);
        } else {
            content.push(c);
            j += 1;
        }
    }
    (content, j)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offenders_in(source: &str) -> Vec<char> {
        literals(source)
            .into_iter()
            .filter(|literal| !MATH_TEXT_ALLOWLIST.contains(&literal.text.as_str()))
            .flat_map(|literal| literal.text.chars().collect::<Vec<_>>())
            .filter(|c| forbidden(*c))
            .collect()
    }

    #[test]
    fn no_symbol_glyphs_in_editor_string_literals() {
        let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let files = walk_rs_files(&src_dir);
        assert!(
            files.len() > 5,
            "scanner found no sources under {src_dir:?}"
        );
        let mut offenders: Vec<String> = Vec::new();
        for entry in files {
            let source = std::fs::read_to_string(&entry).expect("read editor source");
            for literal in literals(&source) {
                if MATH_TEXT_ALLOWLIST.contains(&literal.text.as_str()) {
                    continue;
                }
                for c in literal.text.chars().filter(|c| forbidden(*c)) {
                    let line = source[..literal.offset].matches('\n').count() + 1;
                    offenders.push(format!(
                        "{}:{line}: U+{:04X} in literal {:?}",
                        entry.display(),
                        u32::from(c),
                        literal.text
                    ));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "symbol glyphs must not be used as icons; replace with a Lucide Icon:\n{}",
            offenders.join("\n")
        );
    }

    fn walk_rs_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut files = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return files;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(walk_rs_files(&path));
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
        files
    }

    // Glyphs below are spelled as escapes so this very file stays glyph-free.
    #[test]
    fn scanner_catches_literals_escapes_and_char_literals() {
        let glyph = |code: u32| char::from_u32(code).unwrap();
        let (play, diamond, emoji, warning) =
            (glyph(0x25B6), glyph(0x25C6), glyph(0x1F501), glyph(0x26A0));
        let source = format!(
            "let a = \"{play}\"; let b = '{diamond}'; let c = \"\\u{{26A0}}\"; \
             let d = r#\"{emoji}\"#; let e = b\"x\";"
        );
        let found = offenders_in(&source);
        assert_eq!(found, vec![play, diamond, warning, emoji]);
    }

    #[test]
    fn scanner_ignores_comments_lifetimes_and_plain_text() {
        let arrow = char::from_u32(0x2192).unwrap();
        let source = format!(
            "// quote \" and {arrow}\n/// doc \"{arrow}\n/* {arrow} \" */\n\
             fn f<'a>(x: &'a str) -> &'static str {{ \"plain {}\" }}",
            char::from_u32(0x2014).unwrap()
        );
        assert_eq!(offenders_in(&source).len(), 0);
        // The lifetime above must not swallow the string that follows it.
        let lits = literals(&source);
        assert_eq!(lits.len(), 1);
        assert!(lits[0].text.starts_with("plain"));
    }

    #[test]
    fn the_range_boundaries_match_the_policy() {
        for code in [
            0x2190, 0x2BFF, 0x1F000, 0x1FAFF, 0x2600, 0x27BF, 0x25A0, 0x25FF,
        ] {
            assert!(forbidden(char::from_u32(code).unwrap()), "U+{code:04X}");
        }
        for code in [0x218F, 0x2C00, 0x1EFFF, 0x1FB00] {
            assert!(!forbidden(char::from_u32(code).unwrap()), "U+{code:04X}");
        }
    }
}
