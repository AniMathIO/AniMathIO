use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::time::Duration;

use serde_json::Value;

use crate::AssistError;
use crate::output::clean_latex;
use crate::prompt::{check_input, request_body};
use crate::settings::Endpoint;

/// How long to wait for the server to accept the connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// The default time one conversion may take, local models included.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
/// How often a waiting conversion looks at its [`CancelToken`].
const CANCEL_POLL: Duration = Duration::from_millis(25);
/// The largest response body read, in bytes.
const MAX_RESPONSE_BYTES: u64 = 1 << 20;
/// The longest server error message kept for the dialog, in characters.
const MAX_DETAIL_CHARS: usize = 200;

/// Aborts a conversion that is in flight. Clone it, hand one clone to the worker that runs
/// [`Converter::convert`] and keep the other to call [`CancelToken::cancel`] from the UI thread.
#[derive(Clone, Default)]
pub struct CancelToken(Arc<CancelState>);

#[derive(Default)]
struct CancelState {
    cancelled: AtomicBool,
    agent: Mutex<Option<ureq::Agent>>,
}

impl CancelToken {
    /// A token that has not been cancelled.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Cancel: a request blocked on the network returns [`AssistError::Cancelled`] straight
    /// away (the connection is closed), and a later request with this token never starts.
    pub fn cancel(&self) {
        let agent = {
            let mut slot = self.0.agent.lock().unwrap_or_else(PoisonError::into_inner);
            self.0.cancelled.store(true, Ordering::SeqCst);
            slot.take()
        };
        if let Some(agent) = agent {
            agent.shutdown();
        }
    }

    /// Whether [`CancelToken::cancel`] was called.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::SeqCst)
    }

    /// Register the agent whose connections `cancel` must close; fails when already cancelled.
    fn attach(&self, agent: &ureq::Agent) -> Result<(), AssistError> {
        let mut slot = self.0.agent.lock().unwrap_or_else(PoisonError::into_inner);
        if self.is_cancelled() {
            return Err(AssistError::Cancelled);
        }
        *slot = Some(agent.clone());
        Ok(())
    }

    fn detach(&self) {
        self.0
            .agent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
    }
}

impl fmt::Debug for CancelToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CancelToken")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

/// Something that can turn a description into a model reply. [`HttpConverter`] is the real
/// implementation; the editor takes it as a trait object so tests inject a mock.
pub trait Converter: Send + Sync + fmt::Debug {
    /// Send `prose` to the model behind `endpoint` and return its raw reply. Blocks; call it
    /// from a worker thread. Must return [`AssistError::Cancelled`] soon after `cancel` fires.
    fn complete(
        &self,
        endpoint: &Endpoint,
        prose: &str,
        cancel: &CancelToken,
    ) -> Result<String, AssistError>;

    /// [`Converter::complete`] plus input checks and [`clean_latex`]: plain LaTeX, or an error.
    fn convert(
        &self,
        endpoint: &Endpoint,
        prose: &str,
        cancel: &CancelToken,
    ) -> Result<String, AssistError> {
        let prose = check_input(prose)?;
        let reply = self.complete(endpoint, prose, cancel)?;
        if cancel.is_cancelled() {
            return Err(AssistError::Cancelled);
        }
        clean_latex(&reply)
    }
}

/// The OpenAI-compatible `chat/completions` client (blocking, rustls, no redirects).
#[derive(Clone, Debug)]
pub struct HttpConverter {
    timeout: Duration,
}

impl Default for HttpConverter {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

impl HttpConverter {
    /// A client with the default 60 second limit.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Change the overall limit for one request.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    fn agent(&self, endpoint: &Endpoint) -> ureq::Agent {
        let mut builder = ureq::Agent::config_builder()
            .user_agent(format!("AniMathIO/{}", env!("CARGO_PKG_VERSION")))
            .http_status_as_error(false)
            .max_redirects(0)
            .https_only(false)
            .timeout_connect(Some(CONNECT_TIMEOUT.min(self.timeout)))
            .timeout_global(Some(self.timeout));
        if endpoint.target.loopback {
            // A server on this machine is reached directly, whatever proxy the environment
            // names. Every other host, a LAN server included, honours the proxy settings.
            builder = builder.proxy(None);
        }
        builder.build().into()
    }

    fn timeout_secs(&self) -> u64 {
        self.timeout.as_secs().max(1)
    }
}

impl Converter for HttpConverter {
    fn complete(
        &self,
        endpoint: &Endpoint,
        prose: &str,
        cancel: &CancelToken,
    ) -> Result<String, AssistError> {
        let agent = self.agent(endpoint);
        cancel.attach(&agent)?;
        let result = {
            let converter = self.clone();
            let worker_agent = agent.clone();
            let endpoint = endpoint.clone();
            let prose = prose.to_owned();
            run_cancellable(cancel, move || {
                converter.exchange(&worker_agent, &endpoint, &prose)
            })
        };
        cancel.detach();
        match result {
            Err(_) if cancel.is_cancelled() => Err(AssistError::Cancelled),
            other => other,
        }
    }
}

/// Run `work` on its own thread and wait for it, looking at `cancel` every [`CANCEL_POLL`].
///
/// A blocking `ureq` call cannot be interrupted portably: `Agent::shutdown` wakes a blocked
/// read on Linux and macOS but not on Windows, where only closing the socket does. So the
/// caller never waits on the socket: once `cancel` is observed this returns
/// [`AssistError::Cancelled`] at once (after a best-effort `Agent::shutdown`, done by
/// [`CancelToken::cancel`]). The abandoned worker keeps running until the request ends or
/// the global timeout (60 s by default) fires; it owns copies of its inputs and touches no
/// application state, and its result is dropped.
///
/// Rule: the token is checked before every wait, so once `cancel()` has been called this
/// always returns `Cancelled`, even if the result arrived in the meantime. A result that
/// arrived before `cancel()` was called is returned.
fn run_cancellable<T, F>(cancel: &CancelToken, work: F) -> Result<T, AssistError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, AssistError> + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("assist-request".into())
        .spawn(move || {
            // The receiver is gone after a cancel: the result is simply dropped.
            let _ = tx.send(work());
        });
    if let Err(error) = spawned {
        return Err(AssistError::Network(format!(
            "could not start the request: {error}"
        )));
    }
    loop {
        if cancel.is_cancelled() {
            return Err(AssistError::Cancelled);
        }
        match rx.recv_timeout(CANCEL_POLL) {
            Ok(result) => {
                return if cancel.is_cancelled() {
                    Err(AssistError::Cancelled)
                } else {
                    result
                };
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(AssistError::Network("the request thread stopped".into()));
            }
        }
    }
}

impl HttpConverter {
    fn exchange(
        &self,
        agent: &ureq::Agent,
        endpoint: &Endpoint,
        prose: &str,
    ) -> Result<String, AssistError> {
        let body = request_body(&endpoint.model, prose).to_string();
        let mut request = agent
            .post(&endpoint.target.chat_url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json");
        if let Some(key) = &endpoint.api_key {
            // `Endpoint::new` already refuses this; check again where the header is built.
            if !endpoint.target.may_send_key() {
                return Err(AssistError::InsecureKey(endpoint.target.host.clone()));
            }
            request = request.header("Authorization", format!("Bearer {}", key.expose()));
        }
        let mut response = request
            .send(body.as_str())
            .map_err(|error| self.map_error(&error, endpoint))?;
        let status = response.status().as_u16();
        let text = response
            .body_mut()
            .with_config()
            .limit(MAX_RESPONSE_BYTES)
            .read_to_string()
            .map_err(|error| self.map_error(&error, endpoint))?;
        if !(200..300).contains(&status) {
            let detail = bounded(&redact(&error_detail(&text), endpoint));
            return Err(if status == 401 || status == 403 {
                AssistError::Auth { status, detail }
            } else {
                AssistError::Http { status, detail }
            });
        }
        // A 2xx body can carry an error message (or echo the key) just like an error status.
        parse_reply(&text).map_err(|error| match error {
            AssistError::BadResponse(message) => {
                AssistError::BadResponse(bounded(&redact(&message, endpoint)))
            }
            other => other,
        })
    }

    fn map_error(&self, error: &ureq::Error, endpoint: &Endpoint) -> AssistError {
        match error {
            ureq::Error::Timeout(_) => AssistError::Timeout(self.timeout_secs()),
            ureq::Error::Io(io) if io.kind() == std::io::ErrorKind::TimedOut => {
                AssistError::Timeout(self.timeout_secs())
            }
            ureq::Error::BodyExceedsLimit(_) => AssistError::BadResponse("reply too large".into()),
            other => AssistError::Network(bounded(&redact(&other.to_string(), endpoint))),
        }
    }
}

/// The `error.message` of an OpenAI-style error body, or the body itself.
fn error_detail(body: &str) -> String {
    let message = serde_json::from_str::<Value>(body).ok().and_then(|json| {
        let error = json
            .get("error")
            .or_else(|| json.as_array()?.first()?.get("error"))?;
        error
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| error.as_str())
            .map(str::to_owned)
    });
    message.unwrap_or_else(|| body.to_owned())
}

/// A server message on one line, at most [`MAX_DETAIL_CHARS`] characters. Run [`redact`]
/// first: cutting a message can leave a prefix of the key that `redact` would no longer
/// recognise.
fn bounded(text: &str) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut short: String = text.chars().take(MAX_DETAIL_CHARS).collect();
    if text.chars().count() > MAX_DETAIL_CHARS {
        short.push('…');
    }
    short
}

/// Never let the API key into a message, even if a server echoes it back.
fn redact(text: &str, endpoint: &Endpoint) -> String {
    match &endpoint.api_key {
        Some(key) => text.replace(key.expose(), "***"),
        None => text.to_owned(),
    }
}

/// The assistant text of a chat-completions response.
fn parse_reply(body: &str) -> Result<String, AssistError> {
    let json: Value = serde_json::from_str(body)
        .map_err(|_| AssistError::BadResponse("the body is not JSON".into()))?;
    if let Some(error) = json.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| error.as_str())
            .unwrap_or("unknown error");
        return Err(AssistError::BadResponse(message.to_owned()));
    }
    let content = json.pointer("/choices/0/message/content").ok_or_else(|| {
        AssistError::BadResponse("there is no `choices[0].message.content`".into())
    })?;
    match content {
        Value::String(text) => Ok(text.clone()),
        // Some servers return content parts.
        Value::Array(parts) => Ok(parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("")),
        _ => Err(AssistError::BadResponse("the reply has no text".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_string_and_part_content() {
        let body = r#"{"choices":[{"message":{"role":"assistant","content":"x^2"}}]}"#;
        assert_eq!(parse_reply(body).unwrap(), "x^2");
        let parts = r#"{"choices":[{"message":{"content":[{"type":"text","text":"a"},{"type":"text","text":"b"}]}}]}"#;
        assert_eq!(parse_reply(parts).unwrap(), "ab");
        for bad in [
            "nope",
            "{}",
            r#"{"choices":[]}"#,
            r#"{"choices":[{"message":{"content":null}}]}"#,
        ] {
            assert!(
                matches!(parse_reply(bad), Err(AssistError::BadResponse(_))),
                "{bad}"
            );
        }
        assert!(matches!(
            parse_reply(r#"{"error":{"message":"model overloaded"}}"#),
            Err(AssistError::BadResponse(m)) if m == "model overloaded"
        ));
    }

    #[test]
    fn error_detail_prefers_the_message_and_is_short() {
        assert_eq!(
            error_detail(r#"{"error":{"message":"Incorrect API key","type":"x"}}"#),
            "Incorrect API key"
        );
        assert_eq!(
            bounded(&error_detail("<html>\n  Bad gateway </html>")),
            "<html> Bad gateway </html>"
        );
        assert!(bounded(&"a".repeat(1000)).chars().count() <= MAX_DETAIL_CHARS + 1);
    }

    #[test]
    fn a_key_cut_by_the_length_cap_is_still_redacted() {
        let settings = crate::AssistSettings {
            base_url: "https://api.example.test/v1".into(),
            model: "m".into(),
        };
        let key = "sk-0123456789abcdef";
        let endpoint = Endpoint::new(&settings, crate::Secret::new(key)).unwrap();
        // The key straddles the 200 character cap: redact before cutting, not after.
        let text = format!(
            "{}{key}{}",
            "a".repeat(MAX_DETAIL_CHARS - 5),
            "b".repeat(50)
        );
        let shown = bounded(&redact(&text, &endpoint));
        assert!(!shown.contains("sk-"), "{shown}");
        assert!(!shown.contains(&key[..4]), "{shown}");
    }

    #[test]
    fn transport_errors_are_redacted_and_bounded() {
        let settings = crate::AssistSettings {
            base_url: "https://api.example.test/v1".into(),
            model: "m".into(),
        };
        let key = "sk-0123456789abcdef";
        let endpoint = Endpoint::new(&settings, crate::Secret::new(key)).unwrap();
        let message = format!("connection reset by {key}\n{}", "x".repeat(1000));
        let error = ureq::Error::Io(std::io::Error::other(message));
        let AssistError::Network(shown) = HttpConverter::new().map_error(&error, &endpoint) else {
            panic!("a transport error maps to Network");
        };
        assert!(!shown.contains(key) && !shown.contains("sk-"), "{shown}");
        assert!(shown.chars().count() <= MAX_DETAIL_CHARS + 1, "{shown}");
        assert!(!shown.contains('\n'));
    }

    #[test]
    fn cancel_returns_promptly_while_the_worker_ignores_it() {
        use std::time::Instant;
        let token = CancelToken::new();
        let (release, blocked) = mpsc::channel::<()>();
        let canceller = {
            let token = token.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(100));
                token.cancel();
            })
        };
        let started = Instant::now();
        // The worker blocks on something no socket shutdown can wake.
        let result = run_cancellable(&token, move || {
            let _ = blocked.recv();
            Ok("late".to_owned())
        });
        assert_eq!(result, Err(AssistError::Cancelled));
        assert!(started.elapsed() < Duration::from_secs(2));
        canceller.join().unwrap();
        drop(release);
    }

    #[test]
    fn a_result_before_cancel_is_returned_and_cancel_after_wins() {
        let token = CancelToken::new();
        assert_eq!(run_cancellable(&token, || Ok(7)), Ok(7));
        assert_eq!(
            run_cancellable(&token, || Err::<u8, _>(AssistError::EmptyInput)),
            Err(AssistError::EmptyInput)
        );
        token.cancel();
        assert_eq!(
            run_cancellable(&token, || Ok(7)),
            Err(AssistError::Cancelled)
        );
    }

    #[test]
    fn a_cancelled_token_refuses_to_start() {
        let token = CancelToken::new();
        token.cancel();
        assert!(token.is_cancelled());
        let agent = ureq::Agent::new_with_defaults();
        assert_eq!(token.attach(&agent), Err(AssistError::Cancelled));
    }
}
