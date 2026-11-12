//! Guard rail: the editor never uses Unicode symbol glyphs as icons.
//!
//! Pictograms are Lucide SVGs via `gpui-component`'s `Icon`, so this module only
//! exists to prove — in a unit test — that no string literal in the editor
//! source contains a character from the emoji/symbol ranges that used to stand
//! in for icons (`▶`, `◆`, `⚠`, `↻`, `●`, `−`, …).

/// Characters that must not appear as UI icons in string literals.
#[cfg(test)]
fn forbidden(c: char) -> bool {
    let code = u32::from(c);
    (0x2190..=0x2BFF).contains(&code) // arrows, math operators, misc symbols, blocks
        || (0x1F000..=0x1FAFF).contains(&code) // emoji & pictographs
        || (0x2600..=0x27BF).contains(&code) // misc symbols & dingbats
        || (0x25A0..=0x25FF).contains(&code) // geometric shapes
}

/// Extract the contents of `"..."`, `'...'`, `r"..."` and (approximately)
/// `r#"..."#` literals, with escape sequences kept as their source spelling so a
/// literal `◆` is still caught. Byte order markers and line continuations are
/// not expected in the editor source.
#[cfg(test)]
fn literals(source: &str) -> Vec<(usize, String)> {
    let chars: Vec<char> = source.chars().collect();
    let mut found = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '"' => {
                let (content, next) = read_quoted(&chars, i + 1, '"');
                found.push((i, content));
                i = next;
            }
            '\'' => {
                let (content, next) = read_quoted(&chars, i + 1, '\'');
                found.push((i, content));
                i = next;
            }
            'r' if chars.get(i + 1) == Some(&'"') => {
                // Raw string r"…" has no escape sequences.
                let mut content = String::new();
                let mut j = i + 2;
                while j < chars.len() && chars[j] != '"' {
                    content.push(chars[j]);
                    j += 1;
                }
                found.push((i, content));
                i = j + 1;
            }
            _ => i += 1,
        }
    }
    found
}

#[cfg(test)]
fn read_quoted(chars: &[char], start: usize, quote: char) -> (String, usize) {
    let mut content = String::new();
    let mut j = start;
    while j < chars.len() {
        match chars[j] {
            '\\' => {
                content.push('\\');
                if let Some(escaped) = chars.get(j + 1) {
                    content.push(*escaped);
                    j += 2;
                } else {
                    j += 1;
                }
            }
            c if c == quote => {
                return (content, j + 1);
            }
            c => {
                content.push(c);
                j += 1;
            }
        }
    }
    (content, j)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_symbol_glyphs_in_editor_string_literals() {
        let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut offenders: Vec<String> = Vec::new();
        for entry in walk_rs_files(&src_dir) {
            let source = std::fs::read_to_string(&entry).expect("read editor source");
            for (offset, literal) in literals(&source) {
                for c in literal.chars() {
                    if forbidden(c) {
                        let line = source[..offset].matches('\n').count() + 1;
                        offenders.push(format!(
                            "{}:{line}: U+{:04X} '{}' in literal {:?}",
                            entry.display(),
                            u32::from(c),
                            c,
                            truncate(&literal)
                        ));
                    }
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "symbol glyphs must not be used as icons; replace with gpui-component Icon:\n{}",
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

    fn truncate(literal: &str) -> String {
        let mut chars = literal.chars();
        let head: String = chars.by_ref().take(24).collect();
        if chars.next().is_some() {
            format!("{head}…")
        } else {
            head
        }
    }

    #[test]
    fn scanner_extracts_literals_and_ignores_comments() {
        // Glyphs are spelled as escapes so this very file stays glyph-free.
        let source = format!(
            "// comment {}\nlet _ = \"plain\"; let _ = \"{}\";",
            '\u{25B6}', '\u{25C6}'
        );
        let literals = literals(&source);
        assert_eq!(literals.len(), 2);
        assert_eq!(literals[0].1, "plain");
        assert_eq!(literals[1].1, "\u{25C6}");
    }
}
