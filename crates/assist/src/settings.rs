use std::fmt;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::Path;

use serde::{Deserialize, Serialize};
use ureq::http::Uri;

use crate::AssistError;

/// A credential that must never reach logs, settings files or error messages.
///
/// `Debug` prints a placeholder; the value is only reachable through [`Secret::expose`], which
/// the HTTP client uses to build the `Authorization` header.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Wrap a credential. Surrounding whitespace is dropped; an empty value is no secret.
    #[must_use]
    pub fn new(value: &str) -> Option<Self> {
        let value = value.trim();
        (!value.is_empty()).then(|| Self(value.to_owned()))
    }

    /// The credential itself. Use only to build a request.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

/// A ready-made provider configuration offered as a starting point in the setup form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preset {
    /// Short button label.
    pub name: &'static str,
    /// OpenAI-compatible base URL.
    pub base_url: &'static str,
    /// A model that exists on that service at the time of writing; the user can change it.
    pub model: &'static str,
    /// Whether the service needs an API key.
    pub needs_key: bool,
}

/// `OpenAI`, Gemini's OpenAI-compatible endpoint and two local servers.
pub const PRESETS: &[Preset] = &[
    Preset {
        name: "OpenAI",
        base_url: "https://api.openai.com/v1",
        model: "gpt-4.1-mini",
        needs_key: true,
    },
    Preset {
        name: "Gemini",
        base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
        model: "gemini-2.5-flash",
        needs_key: true,
    },
    Preset {
        name: "Ollama (local)",
        base_url: "http://localhost:11434/v1",
        model: "llama3.2",
        needs_key: false,
    },
    Preset {
        name: "llama.cpp (local)",
        base_url: "http://localhost:8080/v1",
        model: "default",
        needs_key: false,
    },
];

/// The non-secret AI assist configuration, stored as `assist.json` in the editor config
/// directory. There is deliberately no key field: the API key lives in the system keyring.
/// An empty base URL means the feature is off.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AssistSettings {
    /// OpenAI-compatible base URL, for example `https://api.openai.com/v1`.
    pub base_url: String,
    /// Model name sent in the request.
    pub model: String,
}

/// Where a request would go, derived from the settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Target {
    pub(crate) chat_url: String,
    pub(crate) host: String,
    /// `scheme://host:port` with the default port written out; the identity a key belongs to.
    pub(crate) origin: String,
    pub(crate) https: bool,
    /// `localhost`, `127.0.0.0/8` and `::1`: the only hosts that may be reached over plain
    /// `http://` with a key and the only ones that bypass the proxy.
    pub(crate) loopback: bool,
    pub(crate) local: bool,
}

impl Target {
    /// Whether an `Authorization` header may go to this server: never over plain `http://`
    /// to a host that is not on this machine.
    pub(crate) fn may_send_key(&self) -> bool {
        self.https || self.loopback
    }
}

/// Whether the API key matters for a server, which decides whether the editor looks it up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyUse {
    /// An online service over `https://`: it needs a key.
    Required,
    /// A server on this machine or network over a connection that may carry a key
    /// (loopback `http://`, or `https://`): most need none.
    Optional,
    /// A network server over plain `http://`: a key would travel unencrypted, so none is ever
    /// stored or sent.
    Never,
}

impl AssistSettings {
    /// Whether a base URL and a model are both set. The feature is off otherwise.
    #[must_use]
    pub fn is_configured(&self) -> bool {
        !self.base_url.trim().is_empty() && !self.model.trim().is_empty()
    }

    /// The host (with a non-default port) requests go to, for the "what is sent where" notice.
    pub fn host(&self) -> Result<String, AssistError> {
        self.target().map(|target| target.host)
    }

    /// The identity the API key is stored under: `scheme://host:port` with the default port
    /// written out (`https://api.openai.com:443`), so the `http` and `https` servers of one
    /// host and port, or two ports of one host, never share a key.
    pub fn origin(&self) -> Result<String, AssistError> {
        self.target().map(|target| target.origin)
    }

    /// Whether this server needs, may take or must never be given an API key.
    pub fn key_use(&self) -> Result<KeyUse, AssistError> {
        self.target().map(|target| target.key_use())
    }

    /// Whether the server is on this machine or the local network (no internet involved).
    pub fn is_local(&self) -> Result<bool, AssistError> {
        self.target().map(|target| target.local)
    }

    pub(crate) fn target(&self) -> Result<Target, AssistError> {
        if !self.is_configured() {
            return Err(AssistError::NotConfigured);
        }
        let base = self.base_url.trim().trim_end_matches('/');
        let uri: Uri = base
            .parse()
            .map_err(|_| AssistError::InvalidBaseUrl("it is not a URL".into()))?;
        let scheme = uri.scheme_str().unwrap_or_default();
        if scheme != "http" && scheme != "https" {
            return Err(AssistError::InvalidBaseUrl(
                "it must start with https:// or http://".into(),
            ));
        }
        let authority = uri
            .authority()
            .ok_or_else(|| AssistError::InvalidBaseUrl("it has no host".into()))?;
        if authority.as_str().contains('@') {
            return Err(AssistError::InvalidBaseUrl(
                "put the API key in the key field, not in the URL".into(),
            ));
        }
        if uri.query().is_some() || base.contains('#') {
            return Err(AssistError::InvalidBaseUrl(
                "it must not contain a query or fragment".into(),
            ));
        }
        let host = uri.host().unwrap_or_default();
        let local = is_local_host(host);
        let https = scheme == "https";
        if !https && !local {
            return Err(AssistError::InvalidBaseUrl(
                "remote servers must use https:// so your text and key are encrypted".into(),
            ));
        }
        let chat_url = if uri.path().ends_with("/chat/completions") {
            base.to_owned()
        } else {
            format!("{base}/chat/completions")
        };
        let port = uri.port_u16().unwrap_or(if https { 443 } else { 80 });
        Ok(Target {
            chat_url,
            host: authority.as_str().to_ascii_lowercase(),
            origin: format!("{scheme}://{}:{port}", host.to_ascii_lowercase()),
            https,
            loopback: is_loopback_host(host),
            local,
        })
    }

    /// Read the settings; a missing or unreadable file means "not configured".
    #[must_use]
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Write the settings atomically (temporary file in the same directory, then rename).
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }
}

/// A validated settings snapshot plus the optional API key: everything one request needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub(crate) target: Target,
    pub(crate) model: String,
    pub(crate) api_key: Option<Secret>,
}

impl Target {
    fn key_use(&self) -> KeyUse {
        if !self.may_send_key() {
            KeyUse::Never
        } else if self.local {
            KeyUse::Optional
        } else {
            KeyUse::Required
        }
    }
}

impl Endpoint {
    /// Validate `settings` for a request. `api_key` comes from the system keyring and is
    /// optional (local servers need none). A key together with a plain `http://` server that
    /// is not on this machine is refused with [`AssistError::InsecureKey`]: the key would
    /// cross the network unencrypted.
    pub fn new(settings: &AssistSettings, api_key: Option<Secret>) -> Result<Self, AssistError> {
        let target = settings.target()?;
        if api_key.is_some() && !target.may_send_key() {
            return Err(AssistError::InsecureKey(target.host));
        }
        Ok(Self {
            target,
            model: settings.model.trim().to_owned(),
            api_key,
        })
    }

    /// The identity the key is stored under, see [`AssistSettings::origin`].
    #[must_use]
    pub fn origin(&self) -> &str {
        &self.target.origin
    }

    /// Whether a key matters for this server, see [`KeyUse`].
    #[must_use]
    pub fn key_use(&self) -> KeyUse {
        self.target.key_use()
    }

    /// The host the request goes to (`api.openai.com`, `localhost:11434`).
    #[must_use]
    pub fn host(&self) -> &str {
        &self.target.host
    }

    /// The model name.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Whether a key will be sent.
    #[must_use]
    pub fn has_key(&self) -> bool {
        self.api_key.is_some()
    }
}

/// `localhost`, `127.0.0.0/8` and `::1`: this machine only. Other names are never trusted to
/// resolve to it.
fn is_loopback_host(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    host == "localhost" || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// `localhost`, loopback, private-range and link-local addresses, and `.local` names: servers
/// reachable without the internet, where plain `http://` is acceptable.
fn is_local_host(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    if host == "localhost"
        || host
            .rsplit_once('.')
            .is_some_and(|(_, tld)| tld == "localhost" || tld == "local")
    {
        return true;
    }
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => is_local_v4(ip),
        Ok(IpAddr::V6(ip)) => is_local_v6(ip),
        Err(_) => false,
    }
}

fn is_local_v4(ip: Ipv4Addr) -> bool {
    ip.is_loopback() || ip.is_private() || ip.is_link_local()
}

fn is_local_v6(ip: Ipv6Addr) -> bool {
    ip.is_loopback()
        || (ip.segments()[0] & 0xfe00) == 0xfc00 // unique local fc00::/7
        || (ip.segments()[0] & 0xffc0) == 0xfe80 // link-local fe80::/10
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(url: &str) -> AssistSettings {
        AssistSettings {
            base_url: url.into(),
            model: "m".into(),
        }
    }

    #[test]
    fn default_is_off() {
        assert!(!AssistSettings::default().is_configured());
        assert_eq!(
            AssistSettings::default().host(),
            Err(AssistError::NotConfigured)
        );
    }

    #[test]
    fn presets_are_valid_endpoints() {
        for preset in PRESETS {
            let target = settings(preset.base_url).target().expect(preset.name);
            assert_eq!(target.local, !preset.needs_key, "{}", preset.name);
            assert!(target.chat_url.ends_with("/chat/completions"));
        }
    }

    #[test]
    fn host_and_chat_url() {
        let target = settings("https://api.openai.com/v1/").target().unwrap();
        assert_eq!(target.host, "api.openai.com");
        assert_eq!(
            target.chat_url,
            "https://api.openai.com/v1/chat/completions"
        );
        assert!(!target.local);
        let target = settings("http://localhost:11434/v1").target().unwrap();
        assert_eq!(target.host, "localhost:11434");
        assert!(target.local);
        // A full endpoint URL is accepted as is.
        let target = settings("http://127.0.0.1:8080/v1/chat/completions")
            .target()
            .unwrap();
        assert_eq!(target.chat_url, "http://127.0.0.1:8080/v1/chat/completions");
    }

    #[test]
    fn rejects_unsafe_or_malformed_urls() {
        for bad in [
            "ftp://example.com/v1",
            "example.com/v1",
            "http://api.openai.com/v1",
            "https://user:pass@api.openai.com/v1",
            "https://api.openai.com/v1?key=abc",
            "http://8.8.8.8/v1",
        ] {
            assert!(
                matches!(settings(bad).target(), Err(AssistError::InvalidBaseUrl(_))),
                "{bad} should be rejected"
            );
        }
        // Plain http is fine on the local network.
        for ok in [
            "http://192.168.1.20:11434/v1",
            "http://10.0.0.5/v1",
            "http://[::1]:8080/v1",
            "http://my-box.local:8080/v1",
        ] {
            assert!(settings(ok).target().is_ok(), "{ok} should be accepted");
        }
    }

    #[test]
    fn origin_writes_out_default_ports_and_separates_schemes() {
        let origin = |url: &str| settings(url).origin().unwrap();
        assert_eq!(
            origin("https://API.OpenAI.com/v1"),
            "https://api.openai.com:443"
        );
        assert_eq!(
            origin("https://api.openai.com:443/v1/"),
            origin("https://api.openai.com/v1")
        );
        assert_eq!(origin("http://localhost/v1"), "http://localhost:80");
        assert_eq!(origin("http://[::1]:8080/v1"), "http://[::1]:8080");
        // The path and a full endpoint URL do not matter, the scheme and port do.
        assert_eq!(
            origin("http://localhost:11434/v1/chat/completions"),
            origin("http://localhost:11434/other")
        );
        let all = [
            "https://example.test:8443/v1",
            "https://example.test/v1",
            "http://localhost:8443/v1",
            "https://localhost:8443/v1",
            "http://localhost:80/v1",
            "https://localhost:443/v1",
            "http://localhost:443/v1",
            "https://localhost:80/v1",
        ];
        let origins: std::collections::BTreeSet<_> = all.iter().map(|url| origin(url)).collect();
        assert_eq!(origins.len(), all.len(), "{origins:?}");
    }

    #[test]
    fn a_key_never_goes_with_plain_http_off_this_machine() {
        let key = || Secret::new("sk-secret");
        // Loopback http and any https are fine.
        for ok in [
            "http://localhost:11434/v1",
            "http://127.0.0.1:8080/v1",
            "http://[::1]:8080/v1",
            "https://api.openai.com/v1",
            "https://192.168.1.20/v1",
        ] {
            assert!(Endpoint::new(&settings(ok), key()).is_ok(), "{ok}");
        }
        // A LAN or .local server over http would carry it in the clear: refused.
        for bad in [
            "http://192.168.1.20:11434/v1",
            "http://10.0.0.5/v1",
            "http://my-box.local:8080/v1",
            "http://[fd00::1]/v1",
        ] {
            let error = Endpoint::new(&settings(bad), key()).unwrap_err();
            assert!(
                matches!(error, AssistError::InsecureKey(_)),
                "{bad}: {error:?}"
            );
            assert!(!error.to_string().contains("sk-secret"));
            assert!(error.to_string().contains("http://"), "{error}");
            // Without a key the same server is fine.
            assert!(Endpoint::new(&settings(bad), None).is_ok(), "{bad}");
            assert_eq!(settings(bad).key_use(), Ok(KeyUse::Never), "{bad}");
        }
        assert_eq!(
            settings("https://api.openai.com/v1").key_use(),
            Ok(KeyUse::Required)
        );
        assert_eq!(
            settings("http://localhost:11434/v1").key_use(),
            Ok(KeyUse::Optional)
        );
        assert_eq!(
            settings("https://nas.local/v1").key_use(),
            Ok(KeyUse::Optional)
        );
    }

    #[test]
    fn persistence_never_contains_a_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("assist.json");
        let saved = settings("https://api.openai.com/v1");
        saved.save(&path).unwrap();
        assert_eq!(AssistSettings::load(&path), saved);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.to_lowercase().contains("key"), "{text}");
        assert!(!dir.path().join("assist.json.tmp").exists());
        // A file that carries an `api_key` (hand edited) is read without keeping it.
        std::fs::write(
            &path,
            r#"{"base_url":"https://x.example/v1","model":"m","api_key":"sk-secret"}"#,
        )
        .unwrap();
        let loaded = AssistSettings::load(&path);
        assert_eq!(loaded.base_url, "https://x.example/v1");
        assert!(!format!("{loaded:?}").contains("sk-secret"));
        // Missing and broken files mean "off".
        std::fs::write(&path, "not json").unwrap();
        assert!(!AssistSettings::load(&path).is_configured());
        assert!(!AssistSettings::load(&dir.path().join("none.json")).is_configured());
    }

    #[test]
    fn secret_is_redacted() {
        let key = Secret::new("  sk-secret ").unwrap();
        assert_eq!(key.expose(), "sk-secret");
        assert!(Secret::new("   ").is_none());
        let endpoint = Endpoint::new(&settings("https://api.openai.com/v1"), Some(key)).unwrap();
        assert!(!format!("{endpoint:?}").contains("sk-secret"));
        assert!(endpoint.has_key());
    }
}
