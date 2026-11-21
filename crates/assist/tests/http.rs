//! The HTTP client against an in-process mock server (loopback only, no network).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use animathio_assist::{
    AssistError, AssistSettings, CancelToken, Converter, Endpoint, HttpConverter, Secret,
};

/// What the mock server does with a request.
#[derive(Clone)]
enum Reply {
    Json(u16, String),
    Raw(String),
    /// Read the request, then never answer (until the test ends).
    Hang,
}

/// One request as the server saw it.
#[derive(Debug)]
struct Seen {
    request_line: String,
    headers: String,
    body: String,
}

impl Seen {
    fn header(&self, name: &str) -> Option<String> {
        self.headers.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name)
                .then(|| value.trim().to_owned())
        })
    }
}

struct Server {
    base_url: String,
    seen: mpsc::Receiver<Seen>,
}

fn read_request(stream: &mut TcpStream) -> Option<Seen> {
    let mut data = Vec::new();
    let mut chunk = [0u8; 1024];
    let header_end = loop {
        if let Some(position) = data.windows(4).position(|window| window == b"\r\n\r\n") {
            break position;
        }
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            return None;
        }
        data.extend_from_slice(&chunk[..read]);
    };
    let head = String::from_utf8_lossy(&data[..header_end]).into_owned();
    let length = head
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0);
    let mut body = data[header_end + 4..].to_vec();
    while body.len() < length {
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    let (request_line, headers) = head.split_once("\r\n").unwrap_or((&head, ""));
    Some(Seen {
        request_line: request_line.to_owned(),
        headers: headers.to_owned(),
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

fn serve(reply: Reply) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
    let (tx, seen) = mpsc::channel();
    thread::spawn(move || {
        let mut held = Vec::new();
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let Some(request) = read_request(&mut stream) else {
                continue;
            };
            let _ = tx.send(request);
            match &reply {
                Reply::Json(status, body) => {
                    let text = format!(
                        "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(text.as_bytes());
                }
                Reply::Raw(text) => {
                    let _ = stream.write_all(text.as_bytes());
                }
                Reply::Hang => held.push(stream),
            }
        }
    });
    Server { base_url, seen }
}

fn completion(content: &str) -> Reply {
    Reply::Json(
        200,
        serde_json::json!({
            "choices": [{ "message": { "role": "assistant", "content": content } }]
        })
        .to_string(),
    )
}

fn endpoint(server: &Server, key: Option<&str>) -> Endpoint {
    let settings = AssistSettings {
        base_url: server.base_url.clone(),
        model: "test-model".into(),
    };
    Endpoint::new(&settings, key.and_then(Secret::new)).unwrap()
}

#[test]
fn success_sends_the_prompt_and_the_key() {
    let server = serve(completion("\\int_0^1 x^2\\,dx"));
    let latex = HttpConverter::new()
        .convert(
            &endpoint(&server, Some("sk-test-key")),
            "the integral of x squared from 0 to 1",
            &CancelToken::new(),
        )
        .unwrap();
    assert_eq!(latex, r"\int_0^1 x^2\,dx");

    let seen = server.seen.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(seen.request_line, "POST /v1/chat/completions HTTP/1.1");
    assert_eq!(seen.header("authorization").unwrap(), "Bearer sk-test-key");
    assert_eq!(seen.header("content-type").unwrap(), "application/json");
    let body: serde_json::Value = serde_json::from_str(&seen.body).unwrap();
    assert_eq!(body["model"], "test-model");
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(
        body["messages"][1]["content"],
        "the integral of x squared from 0 to 1"
    );
    // Only the prose and the fixed prompt go out: nothing else from the project.
    assert_eq!(body["messages"].as_array().unwrap().len(), 2);
}

#[test]
fn a_local_server_needs_no_key() {
    let server = serve(completion("x^2"));
    HttpConverter::new()
        .convert(&endpoint(&server, None), "x squared", &CancelToken::new())
        .unwrap();
    let seen = server.seen.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(seen.header("authorization").is_none());
}

#[test]
fn code_fences_and_delimiters_are_stripped() {
    let server = serve(completion("```latex\n$$\\frac{a}{b}$$\n```"));
    let latex = HttpConverter::new()
        .convert(&endpoint(&server, None), "a over b", &CancelToken::new())
        .unwrap();
    assert_eq!(latex, r"\frac{a}{b}");
}

#[test]
fn auth_errors_are_readable_and_never_echo_the_key() {
    let server = serve(Reply::Json(
        401,
        r#"{"error":{"message":"Incorrect API key provided: sk-test-key."}}"#.into(),
    ));
    let error = HttpConverter::new()
        .convert(
            &endpoint(&server, Some("sk-test-key")),
            "x",
            &CancelToken::new(),
        )
        .unwrap_err();
    let AssistError::Auth { status, .. } = &error else {
        panic!("expected an auth error, got {error:?}");
    };
    assert_eq!(*status, 401);
    let message = error.to_string();
    assert!(message.contains("HTTP 401"), "{message}");
    assert!(message.contains("Incorrect API key"), "{message}");
    assert!(!message.contains("sk-test-key"), "{message}");
    assert!(!format!("{error:?}").contains("sk-test-key"));
}

#[test]
fn other_statuses_report_the_code() {
    for (status, body) in [(429, r#"{"error":{"message":"slow down"}}"#), (500, "oops")] {
        let server = serve(Reply::Json(status, body.into()));
        let error = HttpConverter::new()
            .convert(&endpoint(&server, None), "x", &CancelToken::new())
            .unwrap_err();
        assert!(
            matches!(&error, AssistError::Http { status: s, .. } if *s == status),
            "{error:?}"
        );
        assert!(error.to_string().contains(&format!("HTTP {status}")));
    }
}

#[test]
fn malformed_replies_are_reported() {
    // Not JSON at all.
    let server = serve(Reply::Json(200, "<html>hello</html>".into()));
    let error = HttpConverter::new()
        .convert(&endpoint(&server, None), "x", &CancelToken::new())
        .unwrap_err();
    assert!(matches!(error, AssistError::BadResponse(_)), "{error:?}");

    // A chat completion without choices.
    let server = serve(Reply::Json(200, r#"{"choices":[]}"#.into()));
    let error = HttpConverter::new()
        .convert(&endpoint(&server, None), "x", &CancelToken::new())
        .unwrap_err();
    assert!(matches!(error, AssistError::BadResponse(_)), "{error:?}");

    // A well-formed reply that is not LaTeX.
    let server = serve(completion("Sorry, I cannot help with that request."));
    let error = HttpConverter::new()
        .convert(&endpoint(&server, None), "x", &CancelToken::new())
        .unwrap_err();
    assert!(matches!(error, AssistError::NotLatex(_)), "{error:?}");

    // The model's own refusal sentinel.
    let server = serve(completion("ERROR"));
    let error = HttpConverter::new()
        .convert(&endpoint(&server, None), "x", &CancelToken::new())
        .unwrap_err();
    assert!(matches!(error, AssistError::NotLatex(_)), "{error:?}");
}

#[test]
fn a_silent_server_times_out() {
    let server = serve(Reply::Hang);
    let started = Instant::now();
    let error = HttpConverter::new()
        .with_timeout(Duration::from_millis(400))
        .convert(&endpoint(&server, None), "x", &CancelToken::new())
        .unwrap_err();
    assert!(matches!(error, AssistError::Timeout(_)), "{error:?}");
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn cancel_aborts_a_request_in_flight() {
    let server = serve(Reply::Hang);
    let token = CancelToken::new();
    let worker = {
        let endpoint = endpoint(&server, None);
        let token = token.clone();
        // The default 60 second limit: only the cancel can end this quickly.
        thread::spawn(move || HttpConverter::new().convert(&endpoint, "x", &token))
    };
    // Wait until the server has the request, so the client is blocked reading.
    server.seen.recv_timeout(Duration::from_secs(5)).unwrap();
    let started = Instant::now();
    token.cancel();
    let error = worker.join().unwrap().unwrap_err();
    assert_eq!(error, AssistError::Cancelled);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "cancel took {:?}",
        started.elapsed()
    );
}

#[test]
fn a_token_cancelled_before_the_request_sends_nothing() {
    let server = serve(completion("x"));
    let token = CancelToken::new();
    token.cancel();
    let error = HttpConverter::new()
        .convert(&endpoint(&server, None), "x", &token)
        .unwrap_err();
    assert_eq!(error, AssistError::Cancelled);
    assert!(
        server
            .seen
            .recv_timeout(Duration::from_millis(300))
            .is_err()
    );
}

#[test]
fn redirects_are_not_followed_so_the_key_stays_put() {
    let target = serve(completion("x"));
    let location = format!("{}/stolen", target.base_url);
    let server = serve(Reply::Raw(format!(
        "HTTP/1.1 307 Temporary Redirect\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )));
    let result = HttpConverter::new().convert(
        &endpoint(&server, Some("sk-test-key")),
        "x",
        &CancelToken::new(),
    );
    assert!(result.is_err(), "{result:?}");
    assert!(
        target
            .seen
            .recv_timeout(Duration::from_millis(300))
            .is_err()
    );
}

#[test]
fn bad_input_never_reaches_the_network() {
    let server = serve(completion("x"));
    let converter = HttpConverter::new();
    let token = CancelToken::new();
    assert_eq!(
        converter.convert(&endpoint(&server, None), "   ", &token),
        Err(AssistError::EmptyInput)
    );
    assert!(matches!(
        converter.convert(&endpoint(&server, None), &"a".repeat(5000), &token),
        Err(AssistError::InputTooLong(5000))
    ));
    assert!(
        server
            .seen
            .recv_timeout(Duration::from_millis(300))
            .is_err()
    );
}

#[test]
fn unreachable_servers_are_a_network_error() {
    // Bind and drop to get a port with nothing listening.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let settings = AssistSettings {
        base_url: format!("http://127.0.0.1:{port}/v1"),
        model: "m".into(),
    };
    let error = HttpConverter::new()
        .convert(
            &Endpoint::new(&settings, None).unwrap(),
            "x",
            &CancelToken::new(),
        )
        .unwrap_err();
    assert!(matches!(error, AssistError::Network(_)), "{error:?}");
}
