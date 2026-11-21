use crate::AssistError;

/// The longest LaTeX source accepted from a model, in characters.
const MAX_OUTPUT_CHARS: usize = 4000;

/// Turn a model reply into plain LaTeX for the formula field.
///
/// Reasoning blocks (`<think>…</think>`) and code fences are removed, a pair of surrounding
/// math delimiters (`$…$`, `$$…$$`, `\[…\]`, `\(…\)`) is dropped, and replies that are empty,
/// the `ERROR` sentinel from the system prompt, absurdly long or plain prose are rejected with
/// [`AssistError::NotLatex`]. Whether the result *compiles* is checked by the LaTeX plugin's
/// preview, which the editor shows before anything is inserted.
pub fn clean_latex(reply: &str) -> Result<String, AssistError> {
    let without_thoughts = strip_thinking(reply);
    let unfenced = strip_fence(without_thoughts.trim());
    let latex = strip_delimiters(unfenced.trim()).trim().to_owned();
    if latex.is_empty() {
        return Err(AssistError::NotLatex("the reply was empty".into()));
    }
    if latex == "ERROR" {
        return Err(AssistError::NotLatex(
            "the model could not read that as a mathematical expression".into(),
        ));
    }
    let length = latex.chars().count();
    if length > MAX_OUTPUT_CHARS {
        return Err(AssistError::NotLatex(format!(
            "the reply is too long ({length} characters)"
        )));
    }
    if looks_like_prose(&latex) {
        return Err(AssistError::NotLatex(
            "the reply reads as a sentence, not a formula".into(),
        ));
    }
    Ok(latex)
}

/// Remove `<think>…</think>` blocks that reasoning models put before the answer (an unclosed
/// block drops the rest).
fn strip_thinking(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("<think>") {
        out.push_str(&rest[..start]);
        match rest[start..].find("</think>") {
            Some(end) => rest = &rest[start + end + "</think>".len()..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// The inside of the first fenced block, or the text itself without one. An unclosed fence
/// keeps everything after it. A fence opened and closed on one line (```` ```x^2``` ````) is
/// the text between the backticks.
fn strip_fence(text: &str) -> &str {
    let Some(open) = text.find("```") else {
        return text;
    };
    let after = &text[open + 3..];
    let first_line = after.split_once('\n').map_or(after, |(line, _)| line);
    if let Some(close) = first_line.find("```") {
        let inner = after[..close].trim();
        if matches!(inner, "latex" | "tex") {
            return "";
        }
        // A language tag may precede the formula (```latex x^2```).
        return ["latex", "tex"]
            .iter()
            .find_map(|tag| {
                inner
                    .strip_prefix(tag)
                    .filter(|rest| rest.starts_with(char::is_whitespace))
            })
            .unwrap_or(inner);
    }
    // The rest of the opening line is the language tag (`latex`, `tex`, ...).
    let body = after.split_once('\n').map_or("", |(_, body)| body);
    body.find("```").map_or(body, |close| &body[..close])
}

fn strip_delimiters(text: &str) -> &str {
    for (open, close) in [("$$", "$$"), ("\\[", "\\]"), ("\\(", "\\)"), ("$", "$")] {
        if let Some(inner) = text
            .strip_prefix(open)
            .and_then(|rest| rest.strip_suffix(close))
        {
            // `$a$ and $b$` and `\[a\] + \[b\]` are two formulas, not one wrapped one.
            if !inner.contains(open) && !inner.contains(close) {
                return inner.trim();
            }
        }
    }
    text
}

/// Several ordinary words and no LaTeX or operator characters: an apology or an explanation.
fn looks_like_prose(text: &str) -> bool {
    if text.contains(['\\', '^', '_', '=', '{', '}', '+', '<', '>']) {
        return false;
    }
    let words = text
        .split_whitespace()
        .filter(|word| {
            word.trim_matches(|c: char| !c.is_alphabetic())
                .chars()
                .count()
                >= 3
                && word
                    .chars()
                    .all(|c| c.is_alphabetic() || ".,;:'!?".contains(c))
        })
        .count();
    words >= 4
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_latex_passes_through() {
        assert_eq!(
            clean_latex("  x^2 + y^2 = z^2\n").unwrap(),
            "x^2 + y^2 = z^2"
        );
        assert_eq!(clean_latex(r"\frac{a}{b}").unwrap(), r"\frac{a}{b}");
        assert_eq!(clean_latex("a b c d").unwrap(), "a b c d");
    }

    #[test]
    fn fences_are_stripped() {
        assert_eq!(
            clean_latex("```latex\n\\int_0^1 x\\,dx\n```").unwrap(),
            r"\int_0^1 x\,dx"
        );
        assert_eq!(clean_latex("```\nx^2\n```\n").unwrap(), "x^2");
        assert_eq!(clean_latex("```tex\nx^2").unwrap(), "x^2");
        // Chatter around the fence is dropped.
        assert_eq!(
            clean_latex("Here is the LaTeX:\n```latex\nE = mc^2\n```\nHope it helps!").unwrap(),
            "E = mc^2"
        );
    }

    #[test]
    fn delimiters_and_reasoning_are_stripped() {
        assert_eq!(clean_latex("$x^2$").unwrap(), "x^2");
        assert_eq!(clean_latex("$$\\sum_{i} i$$").unwrap(), r"\sum_{i} i");
        assert_eq!(clean_latex(r"\[ a^2 \]").unwrap(), "a^2");
        assert_eq!(clean_latex(r"\( a_1 \)").unwrap(), "a_1");
        assert_eq!(
            clean_latex("<think>the user wants x</think>\n```latex\nx^2\n```").unwrap(),
            "x^2"
        );
        // Two separate inline formulas are left alone, whatever the delimiter.
        assert_eq!(clean_latex("$a$ and $b$").unwrap(), "$a$ and $b$");
        assert_eq!(clean_latex(r"\[a\] + \[b\]").unwrap(), r"\[a\] + \[b\]");
        assert_eq!(clean_latex(r"\(a\) and \(b\)").unwrap(), r"\(a\) and \(b\)");
        assert_eq!(clean_latex(r"\[ \frac{a}{b} \]").unwrap(), r"\frac{a}{b}");
    }

    #[test]
    fn one_line_fences_keep_their_content() {
        assert_eq!(clean_latex("```x^2```").unwrap(), "x^2");
        assert_eq!(clean_latex("  ```x^2```  ").unwrap(), "x^2");
        assert_eq!(clean_latex("```latex x^2```").unwrap(), "x^2");
        assert_eq!(clean_latex("```$x^2$```").unwrap(), "x^2");
        assert_eq!(
            clean_latex("Answer: ```\\frac{a}{b}``` done").unwrap(),
            r"\frac{a}{b}"
        );
        // A language tag alone is not a formula.
        assert!(clean_latex("```latex```").is_err());
        // Multi-line fences still work.
        assert_eq!(clean_latex("```latex\nx^2\n```").unwrap(), "x^2");
    }

    #[test]
    fn non_latex_is_rejected() {
        for bad in [
            "",
            "   \n",
            "```\n```",
            "ERROR",
            "Sorry, I can't help with that request.",
            "I am not able to convert this into a formula",
            "<think>never finished",
        ] {
            assert!(
                matches!(clean_latex(bad), Err(AssistError::NotLatex(_))),
                "{bad:?} should be rejected"
            );
        }
        let long = "x+".repeat(3000);
        assert!(matches!(clean_latex(&long), Err(AssistError::NotLatex(_))));
    }
}
