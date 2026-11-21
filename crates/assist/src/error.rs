/// Everything that can go wrong between pressing Convert and having LaTeX. The messages are
/// written for the dialog and never contain the API key.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AssistError {
    /// No base URL or model has been configured.
    #[error("AI assist is not set up. Enter a base URL and a model first.")]
    NotConfigured,
    /// The configured base URL is not usable.
    #[error("The base URL is not valid: {0}")]
    InvalidBaseUrl(String),
    /// The description to convert was empty.
    #[error("Type a description of the formula first.")]
    EmptyInput,
    /// The description is longer than [`MAX_INPUT_CHARS`](crate::MAX_INPUT_CHARS).
    #[error("The description is too long ({0} characters); shorten it.")]
    InputTooLong(usize),
    /// The server rejected the credentials (HTTP 401 or 403).
    #[error("The server rejected the API key (HTTP {status}){detail}", detail = detail_suffix(.detail))]
    Auth {
        /// HTTP status code.
        status: u16,
        /// The server's message, when it sent one.
        detail: String,
    },
    /// The server answered with another error status.
    #[error("The server returned HTTP {status}{detail}", detail = detail_suffix(.detail))]
    Http {
        /// HTTP status code.
        status: u16,
        /// The server's message, when it sent one.
        detail: String,
    },
    /// A saved key would have gone over plain `http://` to a server that is not on this machine.
    #[error(
        "The API key is not sent to {0} because the connection is plain http://. Use an \
         https:// address, or remove the saved key."
    )]
    InsecureKey(String),
    /// The request did not finish in time.
    #[error("The request timed out after {0} seconds.")]
    Timeout(u64),
    /// The user cancelled the request.
    #[error("Cancelled.")]
    Cancelled,
    /// The server could not be reached.
    #[error("Could not reach the server: {0}")]
    Network(String),
    /// The server answered, but not with a usable chat completion.
    #[error("The server's reply was not a chat completion: {0}")]
    BadResponse(String),
    /// The model answered with something that is not LaTeX.
    #[error("The model did not return LaTeX: {0}")]
    NotLatex(String),
}

fn detail_suffix(detail: &str) -> String {
    if detail.is_empty() {
        String::new()
    } else {
        format!(": {detail}")
    }
}
