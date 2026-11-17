//! Bounded registry transport and install validation; no GPUI or implicit network access.
use std::io::Read as _;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use animathio_plugin_api::{PluginKind, PluginManifestToml, PluginPackage};
use animathio_plugin_host::{CommunityError, CommunityStore, InstallOutcome, MAX_PACKAGE_BYTES};
use serde::Deserialize;
use sha2::Digest as _;

pub(in crate::editor) const INDEX_URL: &str =
    "https://raw.githubusercontent.com/AniMathIO/plugins/main/community-index.json";
const MAX_INDEX: u64 = 2 * 1024 * 1024;
pub(in crate::editor) type IndexFetcher = Arc<dyn Fn() -> Result<Index, String> + Send + Sync>;
pub(in crate::editor) type Downloader =
    Arc<dyn Fn(&str, u64) -> Result<Vec<u8>, String> + Send + Sync>;

#[derive(Clone, Debug, Deserialize)]
pub(in crate::editor) struct Index {
    pub schema_version: u32,
    pub plugins: Vec<Entry>,
    pub delisted: Vec<Delisted>,
}
#[derive(Clone, Debug, Deserialize)]
pub(in crate::editor) struct Delisted {
    pub id: String,
    pub reason: String,
}
#[derive(Clone, Debug, Deserialize)]
pub(in crate::editor) struct Entry {
    pub id: String,
    pub name: String,
    pub author: String,
    pub description: String,
    pub license: String,
    pub source_url: String,
    pub kinds: Vec<PluginKind>,
    pub min_abi: String,
    pub version: String,
    pub download_url: String,
    pub sha256: String,
    pub size: u64,
}
impl Entry {
    pub fn compatible(&self) -> Result<(), String> {
        // The host's manifest validator is the single source of truth for ABI/kind support.
        PluginManifestToml {
            api_version: self.min_abi.clone(),
            id: self.id.clone(),
            name: self.name.clone(),
            version: self.version.clone(),
            description: self.description.clone(),
            kinds: self.kinds.clone(),
            capabilities: vec![],
        }
        .validate()
        .map_err(|e| e.to_string())
    }
    pub fn newer_than(&self, installed: &str) -> bool {
        match (
            semver::Version::parse(&self.version),
            semver::Version::parse(installed),
        ) {
            (Ok(a), Ok(b)) => a.cmp_precedence(&b).is_gt(),
            _ => false,
        }
    }
    pub fn install(
        &self,
        store: &CommunityStore,
        download: &Downloader,
        update: bool,
    ) -> Result<InstallOutcome, String> {
        self.compatible()?;
        if self.id.starts_with("org.animathio.") || self.size == 0 || self.size > MAX_PACKAGE_BYTES
        {
            return Err("Invalid registry plugin id or size".into());
        }
        if !self.download_url.starts_with("https://github.com/")
            || !self.download_url.contains("/releases/download/")
        {
            return Err("Registry download must be a GitHub HTTPS release asset".into());
        }
        store
            .install_url_with(&self.download_url, Some(&self.sha256), update, |url| {
                let bytes = download(url, self.size).map_err(CommunityError::Download)?;
                if bytes.len() as u64 != self.size {
                    return Err(CommunityError::Download("Registry size mismatch".into()));
                }
                // Verify the index hash before parsing the untrusted archive at all;
                // `install_bytes` checks it again before writing.
                let actual = hex_lower(&sha2::Sha256::digest(&bytes));
                if !self.sha256.eq_ignore_ascii_case(&actual) {
                    return Err(CommunityError::HashMismatch {
                        expected: self.sha256.to_ascii_lowercase(),
                        actual,
                    });
                }
                let package = PluginPackage::from_bytes(&bytes)?;
                let m = &package.manifest;
                if m.id != self.id
                    || m.version != self.version
                    || m.api_version != self.min_abi
                    || m.kinds != self.kinds
                {
                    return Err(CommunityError::Download(
                        "Registry package metadata mismatch".into(),
                    ));
                }
                Ok(bytes)
            })
            .map_err(|e| e.to_string())
    }
}
fn hex_lower(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}
fn agent(redirects: u32) -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(format!("AniMathIO/{}", env!("CARGO_PKG_VERSION")))
        .timeout_connect(Some(Duration::from_secs(5)))
        .timeout_global(Some(Duration::from_secs(30)))
        .max_redirects(redirects)
        .https_only(true)
        .build()
        .into()
}
fn read(reader: impl std::io::Read, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("Response exceeds size limit".into());
    }
    Ok(bytes)
}
pub(in crate::editor) fn parse(bytes: &[u8]) -> Result<Index, String> {
    #[derive(Deserialize)]
    struct Schema {
        schema_version: u32,
    }
    let schema: Schema = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if schema.schema_version != 1 {
        return Err("This registry needs a newer AniMathIO".into());
    }
    let index: Index = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    debug_assert_eq!(index.schema_version, 1);
    let mut ids = std::collections::BTreeSet::new();
    if index.plugins.len() > 5000 || index.plugins.iter().any(|p| !ids.insert(&p.id)) {
        return Err("Invalid registry entries".into());
    }
    Ok(index)
}
pub(in crate::editor) fn fetcher() -> IndexFetcher {
    // Per-editor cache: conditional requests never download the unchanged index again.
    let cache = Mutex::new(None::<(Option<String>, Index)>);
    Arc::new(move || {
        if let Some(path) = std::env::var_os("ANIMATHIO_COMMUNITY_INDEX_FILE") {
            let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
            return parse(&read(file, MAX_INDEX)?);
        }
        let mut cache = cache.lock().map_err(|e| e.to_string())?;
        let client = agent(0);
        let mut request = client.get(INDEX_URL);
        if let Some((Some(etag), _)) = &*cache {
            request = request.header("If-None-Match", etag);
        }
        let response = request.call().map_err(|e| e.to_string())?;
        if response.status().as_u16() == 304 {
            return cache
                .as_ref()
                .map(|(_, index)| index.clone())
                .ok_or_else(|| "Empty registry cache".into());
        }
        if response.status().as_u16() != 200 {
            return Err(format!("Registry returned {}", response.status()));
        }
        let etag = response
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let index = parse(&read(response.into_body().into_reader(), MAX_INDEX)?)?;
        *cache = Some((etag, index.clone()));
        Ok(index)
    })
}
pub(in crate::editor) fn downloader() -> Downloader {
    Arc::new(|url, limit| {
        let response = agent(5).get(url).call().map_err(|e| e.to_string())?;
        read(
            response.into_body().into_reader(),
            limit.min(MAX_PACKAGE_BYTES),
        )
    })
}
