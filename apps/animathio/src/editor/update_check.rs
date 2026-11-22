//! Startup check for a newer `AniMathIO` release.
//!
//! At most once every 24 hours ([`THROTTLE`], remembered in the config directory across
//! restarts) the editor asks the GitHub releases API whether a newer 2.x release exists
//! and, if so, pushes the normal toast notice with an action that opens the release page
//! in the browser. No automatic download and no telemetry; every network failure is
//! silent (debug-level log only).
//!
//! The check runs on the production environment only: the headless test harness never
//! reaches this module, benchmark runs skip it, a CI environment (`CI` or
//! `GITHUB_ACTIONS` set) skips it, and users opt out in Preferences ("Check for
//! updates") or by setting `ANIMATHIO_NO_UPDATE_CHECK=1`.
//!
//! Tests inject the network ([`check`]), the clock ([`throttled`]) and the environment
//! ([`disabled_by_environment_with`]); nothing here touches the network in tests.

use std::io::Read as _;
use std::path::Path;
use std::time::Duration;

use gpui::AppContext as _;
use semver::Version;
use serde::{Deserialize, Serialize};

use super::Editor;

/// The releases API endpoint asked at startup.
const RELEASES_URL: &str = "https://api.github.com/repos/AniMathIO/AniMathIO/releases/latest";
/// Setting this to `1` disables the startup check for the process.
const ENV_VAR: &str = "ANIMATHIO_NO_UPDATE_CHECK";
/// Where the last attempt is remembered (`editor_env::config_file`).
pub(super) const STATE_FILE: &str = "update-check.json";
/// How often the editor may ask GitHub for the latest release.
const THROTTLE: Duration = Duration::from_hours(24);
/// Connection timeout: the editor must never wait on this check.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Overall request deadline, including reading the response.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Cap on the releases JSON response.
const MAX_BODY: u64 = 2 * 1024 * 1024;

/// The part of a `releases/latest` answer the notice needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Release {
    /// The release version (from a `v2.x.y` `tag_name`).
    pub(super) version: Version,
    /// The release page opened by the notice's action.
    pub(super) url: String,
}

/// The persisted "last attempt" timestamp, kept across restarts so the throttle holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct CheckState {
    /// Unix seconds of the last reserved attempt (successful or failed).
    last_check: u64,
}

/// `v2.1.0` -> `2.1.0`. Older-major tags (`animathio-v1.8.3`, `v1.2.3`), pre-releases
/// (`v2.1.0-rc.1`) and anything unparseable yield `None`: the check never announces them.
fn parse_tag(tag: &str) -> Option<Version> {
    let rest = tag.strip_prefix('v')?;
    let version = Version::parse(rest).ok()?;
    if version.major != 2 || !version.pre.is_empty() {
        return None;
    }
    Some(version)
}

/// Parse a GitHub `releases/latest` JSON body (`tag_name`, `html_url`).
fn parse_release(body: &str) -> Option<Release> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let tag = value.get("tag_name")?.as_str()?;
    let url = value.get("html_url")?.as_str()?;
    if value.get("prerelease").and_then(serde_json::Value::as_bool) == Some(true)
        || value.get("draft").and_then(serde_json::Value::as_bool) == Some(true)
    {
        return None;
    }
    let version = parse_tag(tag)?;
    let expected = format!("https://github.com/AniMathIO/AniMathIO/releases/tag/{tag}");
    if url != expected {
        return None;
    }
    Some(Release {
        version,
        url: expected,
    })
}

/// Whether `latest` should be announced over the running version.
fn is_newer(latest: &Version, current: &Version) -> bool {
    latest.cmp_precedence(current).is_gt()
}

/// One startup check: ask `fetch` for the latest release and report it when it is
/// newer than `current`. `fetch` is injectable, so tests run without the network.
fn check(current: &Version, fetch: impl FnOnce() -> Option<Release>) -> Option<Release> {
    let release = fetch()?;
    is_newer(&release.version, current).then_some(release)
}

/// The environment side of the opt-out: `ANIMATHIO_NO_UPDATE_CHECK=1`, or a CI
/// environment (`CI`/`GITHUB_ACTIONS` set non-empty), disables the check.
fn disabled_by_environment_with(get: impl Fn(&str) -> Option<String>) -> bool {
    let set = |name: &str| get(name).is_some_and(|value| !value.is_empty());
    get(ENV_VAR).as_deref() == Some("1") || set("CI") || set("GITHUB_ACTIONS")
}

fn allowed(env_allows: bool, opted_in: bool, disabled: bool) -> bool {
    env_allows && opted_in && !disabled
}

fn disabled_by_environment() -> bool {
    disabled_by_environment_with(|name| std::env::var(name).ok())
}

/// At most one check per [`THROTTLE`]: the persisted attempt timestamp decides.
fn throttled(state: Option<CheckState>, now: u64) -> bool {
    state.is_some_and(|state| now.saturating_sub(state.last_check) < THROTTLE.as_secs())
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Reserve the attempt before doing any network work. The OS lock serializes
/// simultaneous editor launches; a crash releases it automatically.
fn reserve_attempt(path: &Path, now: u64) -> Option<()> {
    use std::io::{Seek as _, Write as _};
    std::fs::create_dir_all(path.parent()?).ok()?;
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .ok()?;
    file.try_lock().ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let state = serde_json::from_slice(&bytes).ok();
    if throttled(state, now) {
        return None;
    }
    let bytes = serde_json::to_vec(&CheckState { last_check: now }).ok()?;
    file.rewind().ok()?;
    file.write_all(&bytes).ok()?;
    file.set_len(bytes.len() as u64).ok()?;
    file.sync_all().ok()?;
    Some(())
}

fn run_check(
    path: &Path,
    now: u64,
    current: &Version,
    fetch: impl FnOnce() -> Option<Release>,
) -> Option<Release> {
    reserve_attempt(path, now)?;
    check(current, fetch)
}

/// The request agent: plain `AniMathIO/<version>` user agent (no personal data),
/// short timeouts so the check never slows the editor down.
fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(format!("AniMathIO/{}", env!("CARGO_PKG_VERSION")))
        .timeout_global(Some(REQUEST_TIMEOUT))
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .max_redirects(0)
        .build()
        .into()
}

/// Fetch `releases/latest` from GitHub. Any failure is silent: the notice simply
/// never appears, and only the debug log records why.
fn fetch(agent: &ureq::Agent) -> Option<Release> {
    let response = match agent
        .get(RELEASES_URL)
        .header("Accept", "application/vnd.github+json")
        .call()
    {
        Ok(response) => response,
        Err(error) => {
            log::debug!("update check: request failed: {error}");
            return None;
        }
    };
    if response.status().as_u16() != 200 {
        log::debug!("update check: GitHub answered {}", response.status());
        return None;
    }
    let mut body = response.into_body().into_reader().take(MAX_BODY);
    let mut text = String::new();
    match body.read_to_string(&mut text) {
        Ok(_) => parse_release(&text),
        Err(error) => {
            log::debug!("update check: response read failed: {error}");
            None
        }
    }
}

/// The notice: "`AniMathIO` <version> is available" with an action that opens the
/// release page in the browser.
fn notice(release: &Release) -> gpui_component::notification::Notification {
    let url = release.url.clone();
    crate::style::Toast::info(format!("AniMathIO {} is available", release.version)).action(
        move |_, _, cx| {
            let url = url.clone();
            crate::style::secondary_button("update-check-open-release", cx)
                .label("Open release page")
                .on_click(move |_, _, cx| cx.open_url(&url))
        },
    )
}

/// Decide whether this startup should ask GitHub about a newer release and, if so,
/// start the background request; the answer arrives as the editor's normal toast.
/// Called once while the editor window is built, with its environment and stored
/// preference (`env_allows`, `opted_in`); skipped on the headless test harness,
/// in CI, when the user opted out with `ANIMATHIO_NO_UPDATE_CHECK=1`, and within
/// 24 h of the previous attempt.
pub(super) fn schedule(env_allows: bool, opted_in: bool, cx: &mut gpui::Context<Editor>) {
    if cfg!(test) || !allowed(env_allows, opted_in, disabled_by_environment()) {
        return;
    }
    let Some(path) = crate::editor_env::config_file(STATE_FILE) else {
        return;
    };
    let Ok(current) = Version::parse(env!("CARGO_PKG_VERSION")) else {
        return;
    };
    cx.spawn(async move |this, cx| {
        let outcome = cx
            .background_spawn(
                async move { run_check(&path, now_unix(), &current, || fetch(&agent())) },
            )
            .await;
        let Some(release) = outcome else {
            return;
        };
        if let Err(error) = this.update(cx, |editor, cx| {
            if editor.preferences.check_updates {
                editor.notices.push(notice(&release));
            }
            cx.notify();
        }) {
            log::debug!("update check: window closed before the notice: {error}");
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, url: &str) -> Option<Release> {
        parse_release(&format!(r#"{{"tag_name":"{tag}","html_url":"{url}"}}"#))
    }

    #[test]
    fn tags_parse_or_are_rejected() {
        assert_eq!(
            parse_tag("v2.1.0"),
            Some(Version::new(2, 1, 0)),
            "the normal release tag"
        );
        assert_eq!(parse_tag("v2.10.3"), Some(Version::new(2, 10, 3)));
        assert_eq!(parse_tag("2.1.0"), None, "tags are v-prefixed on this repo");
        assert_eq!(
            parse_tag("animathio-v1.8.3"),
            None,
            "v1 tags never announce"
        );
        assert_eq!(parse_tag("v1.8.3"), None, "major 1 never announces");
        assert_eq!(
            parse_tag("v2.1.0-rc.1"),
            None,
            "pre-releases never announce"
        );
        assert_eq!(parse_tag(""), None);
        assert_eq!(parse_tag("v"), None);
        assert_eq!(parse_tag("vbanana"), None);
        assert_eq!(parse_tag("v2"), None);
        assert_eq!(parse_tag("v3.0.0"), None);
    }

    #[test]
    fn release_bodies_parse_or_are_rejected() {
        let good = release(
            "v2.1.0",
            "https://github.com/AniMathIO/AniMathIO/releases/tag/v2.1.0",
        )
        .expect("a releases/latest body");
        assert_eq!(good.version, Version::new(2, 1, 0));
        assert_eq!(
            good.url,
            "https://github.com/AniMathIO/AniMathIO/releases/tag/v2.1.0"
        );
        assert_eq!(release("animathio-v1.8.3", "https://example.com"), None);
        assert_eq!(release("v3.0.0-beta.2", "https://example.com"), None);
        assert_eq!(parse_release("not json"), None);
        assert_eq!(parse_release(r#"{"tag_name":"v2.1.0"}"#), None, "no url");
        assert_eq!(parse_release("{}"), None);
        assert_eq!(release("v2.1.0", "file:///tmp/untrusted"), None);
        assert_eq!(release("v2.1.0", "https://example.com"), None);
        let body = r#"{"tag_name":"v2.1.0","html_url":"https://github.com/AniMathIO/AniMathIO/releases/tag/v2.1.0","prerelease":true}"#;
        assert_eq!(parse_release(body), None);
    }

    #[test]
    fn only_a_strictly_newer_version_announces() {
        let current = Version::new(2, 1, 0);
        let newer = Version::new(current.major, current.minor + 1, current.patch);
        let fetch_of = |version: Version| {
            let tag = format!("v{version}");
            move || {
                release(
                    &tag,
                    &format!("https://github.com/AniMathIO/AniMathIO/releases/tag/{tag}"),
                )
            }
        };
        assert!(check(&current, fetch_of(newer)).is_some(), "newer");
        assert!(check(&current, fetch_of(current.clone())).is_none(), "same");
        assert!(
            check(&current, fetch_of(Version::new(2, 0, 9))).is_none(),
            "older 2.x"
        );
        assert!(check(&current, fetch_of(Version::new(1, 9, 9))).is_none());
        assert!(
            check(&current, fetch_of(Version::parse("2.1.0+build.2").unwrap())).is_none(),
            "build metadata does not change precedence"
        );
        assert!(check(&current, || None).is_none(), "network failure");
    }

    #[test]
    fn environment_gates_the_check() {
        let none = |_: &str| None::<String>;
        assert!(!disabled_by_environment_with(none), "no variable set");
        assert!(
            disabled_by_environment_with(|name| (name == ENV_VAR).then(|| "1".to_owned())),
            "the documented opt-out variable"
        );
        assert!(
            !disabled_by_environment_with(|name| (name == ENV_VAR).then(|| "0".to_owned())),
            "any other value keeps the check on"
        );
        assert!(
            disabled_by_environment_with(|name| (name == "CI").then(|| "true".to_owned())),
            "CI environments never check"
        );
        assert!(disabled_by_environment_with(|name| (name
            == "GITHUB_ACTIONS")
            .then(|| "1".to_owned())),);
        assert!(
            !disabled_by_environment_with(|name| (name == "CI").then(String::new)),
            "an empty variable is not a CI environment"
        );
    }

    #[test]
    fn one_check_per_day() {
        let day = THROTTLE.as_secs();
        let state = |secs: u64| Some(CheckState { last_check: secs });
        assert!(!throttled(None, 1_000), "first start checks");
        assert!(throttled(state(0), day - 1), "just inside the window");
        assert!(!throttled(state(0), day), "the next day may check");
        assert!(throttled(state(0), day - 1_000), "already checked today");
        assert!(!throttled(state(0), day * 30), "a month later checks");
        assert!(
            throttled(state(day), 1_000),
            "a clock behind the last attempt stays quiet"
        );
    }

    #[test]
    fn opt_out_and_headless_gates() {
        assert!(allowed(true, true, false));
        assert!(!allowed(false, true, false));
        assert!(!allowed(true, false, false));
        assert!(!allowed(true, true, true));
    }

    #[test]
    fn simultaneous_launches_only_reserve_one_attempt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update-check.json");
        let barrier = std::sync::Barrier::new(8);
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        reserve_attempt(&path, 100).is_some()
                    })
                })
                .collect();
            assert_eq!(
                handles
                    .into_iter()
                    .map(|h| usize::from(h.join().unwrap()))
                    .sum::<usize>(),
                1
            );
        });
    }

    #[test]
    fn attempts_are_persisted_before_fetch_even_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update-check.json");
        let current = Version::new(2, 1, 0);
        assert!(run_check(&path, 100, &current, || None).is_none());
        assert!(run_check(&path, 101, &current, || panic!("throttled")).is_none());
        let older = || {
            release(
                "v2.0.0",
                "https://github.com/AniMathIO/AniMathIO/releases/tag/v2.0.0",
            )
        };
        assert!(run_check(&path, 100 + THROTTLE.as_secs(), &current, older).is_none());
        assert!(
            run_check(&path, 101 + THROTTLE.as_secs(), &current, || panic!(
                "throttled"
            ))
            .is_none()
        );
    }
}
