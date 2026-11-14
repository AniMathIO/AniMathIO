//! On-demand Whisper model files (ggml format, from the `ggerganov/whisper.cpp` release on
//! Hugging Face). No model ships with the editor, the installers or the repository.
//!
//! Downloads stream into `<file>.part`, resume with an HTTP `Range` request after an
//! interruption, are verified against a pinned SHA-256 and size, and only then renamed
//! into place, so a model file either is complete and verified or does not exist.

use crate::Error;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

/// Environment variable overriding the model directory (used by tests and portable setups).
pub const MODEL_DIR_ENV: &str = "ANIMATHIO_MODEL_DIR";

/// A supported multilingual Whisper size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ModelSize {
    /// 78 MB, fastest, roughest accuracy.
    Tiny,
    /// 148 MB, good default for CPU use.
    #[default]
    Base,
    /// 488 MB, noticeably better, several times slower.
    Small,
}

/// Static description of one downloadable model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelInfo {
    /// Short name used by the CLI, MCP and editor.
    pub name: &'static str,
    /// File name inside the model directory.
    pub file: &'static str,
    /// Download URL.
    pub url: &'static str,
    /// Pinned SHA-256 of the complete file, lowercase hex.
    pub sha256: &'static str,
    /// Exact file size in bytes.
    pub bytes: u64,
}

/// All supported models, smallest first.
pub const MODELS: [ModelInfo; 3] = [
    ModelInfo {
        name: "tiny",
        file: "ggml-tiny.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin",
        sha256: "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
        bytes: 77_691_713,
    },
    ModelInfo {
        name: "base",
        file: "ggml-base.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin",
        sha256: "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
        bytes: 147_951_465,
    },
    ModelInfo {
        name: "small",
        file: "ggml-small.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin",
        sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
        bytes: 487_601_967,
    },
];

impl ModelSize {
    /// Every size.
    pub const ALL: [Self; 3] = [Self::Tiny, Self::Base, Self::Small];

    /// Parse `tiny`, `base` or `small`.
    pub fn parse(name: &str) -> Result<Self, Error> {
        Self::ALL
            .into_iter()
            .find(|size| size.info().name.eq_ignore_ascii_case(name.trim()))
            .ok_or_else(|| {
                Error::Invalid(format!(
                    "unknown model `{name}` (available: tiny, base, small)"
                ))
            })
    }

    /// Static description.
    #[must_use]
    pub fn info(self) -> &'static ModelInfo {
        &MODELS[match self {
            Self::Tiny => 0,
            Self::Base => 1,
            Self::Small => 2,
        }]
    }

    /// Where this model lives (or would be downloaded to).
    #[must_use]
    pub fn path(self) -> PathBuf {
        models_dir().join(self.info().file)
    }

    /// Whether a complete model file is present. Cheap: checks the exact size only; the
    /// hash is verified when the model is downloaded.
    #[must_use]
    pub fn is_installed(self) -> bool {
        fs::metadata(self.path()).is_ok_and(|m| m.len() == self.info().bytes)
    }

    /// Path of the installed model, or [`Error::ModelMissing`].
    pub fn require(self) -> Result<PathBuf, Error> {
        if self.is_installed() {
            return Ok(self.path());
        }
        let info = self.info();
        Err(Error::ModelMissing {
            model: info.name.to_owned(),
            file: info.file.to_owned(),
            size_mb: (info.bytes + 500_000) / 1_000_000,
            dir: models_dir().display().to_string(),
        })
    }
}

/// Directory holding Whisper models: `$ANIMATHIO_MODEL_DIR` or `<user data dir>/models/whisper`.
#[must_use]
pub fn models_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(MODEL_DIR_ENV).filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    directories::ProjectDirs::from("org", "AniMathIO", "AniMathIO").map_or_else(
        || PathBuf::from("models").join("whisper"),
        |dirs| dirs.data_dir().join("models").join("whisper"),
    )
}

/// Human-readable size, e.g. `148 MB`.
#[must_use]
pub fn display_size(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        #[allow(clippy::cast_precision_loss)]
        let gb = bytes as f64 / 1e9;
        format!("{gb:.1} GB")
    } else {
        format!("{} MB", (bytes + 500_000) / 1_000_000)
    }
}

/// Download progress, in bytes of the model file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DownloadProgress {
    /// Bytes on disk so far (including a resumed prefix).
    pub downloaded: u64,
    /// Total expected bytes.
    pub total: u64,
}

/// Download `size` into the model directory unless it is already installed, reporting
/// progress and honouring `cancel`. Returns the final path.
pub fn download(
    size: ModelSize,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(DownloadProgress),
) -> Result<PathBuf, Error> {
    download_to(size.info(), &models_dir(), cancel, progress)
}

/// [`download`] with an explicit model description and directory (testable without network).
pub fn download_to(
    info: &ModelInfo,
    dir: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(DownloadProgress),
) -> Result<PathBuf, Error> {
    fs::create_dir_all(dir)?;
    let target = dir.join(info.file);
    if fs::metadata(&target).is_ok_and(|m| m.len() == info.bytes) {
        progress(DownloadProgress {
            downloaded: info.bytes,
            total: info.bytes,
        });
        return Ok(target);
    }
    let part = dir.join(format!("{}.part", info.file));
    let mut offset = fs::metadata(&part).map_or(0, |m| m.len());
    if offset > info.bytes {
        fs::remove_file(&part)?;
        offset = 0;
    }
    if offset < info.bytes {
        fetch(info, &part, offset, cancel, progress)?;
    }
    verify_file(&part, info)?;
    // Persist before the rename so a crash can never leave a truncated final file.
    File::open(&part)?.sync_all()?;
    fs::rename(&part, &target)?;
    Ok(target)
}

fn fetch(
    info: &ModelInfo,
    part: &Path,
    offset: u64,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(DownloadProgress),
) -> Result<(), Error> {
    let net = |e: &dyn std::fmt::Display| Error::Download(e.to_string());
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(std::time::Duration::from_secs(20)))
        .timeout_recv_body(Some(std::time::Duration::from_secs(60)))
        .build()
        .into();
    let mut request = agent.get(info.url);
    if offset > 0 {
        request = request.header("Range", format!("bytes={offset}-"));
    }
    let response = request.call().map_err(|e| net(&e))?;
    let resumed = response.status().as_u16() == 206;
    let mut file = if resumed {
        OpenOptions::new().append(true).open(part)?
    } else {
        // The server ignored the Range request (or this is a fresh start): start over.
        File::create(part)?
    };
    let mut downloaded = if resumed { offset } else { 0 };
    let mut reader = response.into_body().into_reader();
    let mut buffer = vec![0u8; 256 * 1024];
    progress(DownloadProgress {
        downloaded,
        total: info.bytes,
    });
    loop {
        if cancel.load(Ordering::Relaxed) {
            // Keep the partial file: the next attempt resumes from it.
            file.flush()?;
            return Err(Error::Cancelled);
        }
        let read = reader.read(&mut buffer).map_err(|e| net(&e))?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])?;
        downloaded += read as u64;
        if downloaded > info.bytes {
            return Err(Error::Download(
                "server sent more data than expected".into(),
            ));
        }
        progress(DownloadProgress {
            downloaded,
            total: info.bytes,
        });
    }
    file.flush()?;
    if downloaded != info.bytes {
        return Err(Error::Download(format!(
            "connection ended after {downloaded} of {} bytes; retry to resume",
            info.bytes
        )));
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// Check size and SHA-256 of `path` against `info`. A corrupt file is deleted so the next
/// attempt starts clean.
pub fn verify_file(path: &Path, info: &ModelInfo) -> Result<(), Error> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    let mut length = 0u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        length += read as u64;
    }
    let actual = hex(&hasher.finalize());
    if length != info.bytes || actual != info.sha256 {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(Error::Checksum {
            expected: info.sha256.to_owned(),
            actual,
        });
    }
    Ok(())
}
