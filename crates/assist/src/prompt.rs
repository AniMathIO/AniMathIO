use serde_json::{Value, json};

use crate::AssistError;

/// The longest description (in characters) that is sent.
pub const MAX_INPUT_CHARS: usize = 2000;

/// The system prompt: LaTeX only, in the form the Math dialog's formula field takes.
pub const SYSTEM_PROMPT: &str = "You convert a description of a mathematical expression into LaTeX. \
Reply with the LaTeX source of the expression only. Do not add explanations, comments, \
markdown, code fences or surrounding math delimiters (no $ signs, no \\[ \\] and no \\begin{equation}). \
Use standard LaTeX math commands. If the request is not about a mathematical expression, \
reply with the single word ERROR.";

/// The chat-completions request body for `prose`.
pub(crate) fn request_body(model: &str, prose: &str) -> Value {
    json!({
        "model": model,
        "temperature": 0.2,
        "max_tokens": 1024,
        "stream": false,
        "messages": [
            { "role": "system", "content": SYSTEM_PROMPT },
            { "role": "user", "content": prose },
        ],
    })
}

/// Validate and normalise the text to send.
pub(crate) fn check_input(prose: &str) -> Result<&str, AssistError> {
    let prose = prose.trim();
    if prose.is_empty() {
        return Err(AssistError::EmptyInput);
    }
    let length = prose.chars().count();
    if length > MAX_INPUT_CHARS {
        return Err(AssistError::InputTooLong(length));
    }
    Ok(prose)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_limits() {
        assert_eq!(check_input("  x  ").unwrap(), "x");
        assert_eq!(check_input(" \n"), Err(AssistError::EmptyInput));
        let long = "a".repeat(MAX_INPUT_CHARS + 1);
        assert_eq!(
            check_input(&long),
            Err(AssistError::InputTooLong(MAX_INPUT_CHARS + 1))
        );
        assert!(check_input(&"a".repeat(MAX_INPUT_CHARS)).is_ok());
    }

    #[test]
    fn request_has_system_prompt_and_only_the_prose() {
        let body = request_body("m", "the integral of x");
        assert_eq!(body["model"], "m");
        assert_eq!(body["stream"], false);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[1]["content"], "the integral of x");
    }
}
