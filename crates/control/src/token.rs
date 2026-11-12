//! Random bearer-token generation, `0600` storage and constant-time compare.

use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use subtle::ConstantTimeEq;

/// Number of random bytes in a control token (256 bits).
const TOKEN_BYTES: usize = 32;

/// Generate a fresh hex-encoded token.
pub(crate) fn generate() -> io::Result<String> {
    let mut bytes = [0_u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes)
        .map_err(|error| io::Error::other(format!("token generation failed: {error}")))?;
    Ok(hex(&bytes))
}

/// Write the token to a file readable only by the current user.
///
/// The token is first written to a unique `0600` temporary file in the same
/// directory (created with `O_EXCL` and, on Unix, `O_NOFOLLOW`), then renamed
/// over the target so a pre-existing symlink is replaced rather than followed.
pub(crate) fn write_file(path: &Path, token: &str) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "token path has no parent directory",
            )
        })?;
    let tmp_path = token_tmp_path(parent)?;

    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
        options.custom_flags(rustix::fs::OFlags::NOFOLLOW.bits().cast_signed());
    }

    let write_result = (|| {
        let mut file = options.open(&tmp_path)?;
        file.write_all(token.as_bytes())?;
        file.flush()
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(&tmp_path);
        return Err(error);
    }

    #[cfg(unix)]
    {
        // Rename atomically replaces any existing path without following it.
        if let Err(error) = fs::rename(&tmp_path, path) {
            let _ = fs::remove_file(&tmp_path);
            return Err(error);
        }
        Ok(())
    }

    #[cfg(windows)]
    {
        // `std::fs::rename` does not replace an existing file on Windows.
        if fs::symlink_metadata(path).is_ok() {
            fs::remove_file(path)?;
        }
        if let Err(error) = fs::rename(&tmp_path, path) {
            let _ = fs::remove_file(&tmp_path);
            return Err(error);
        }
        Ok(())
    }
}

/// Read a token file, trimming surrounding whitespace.
///
/// On Unix the file is verified to be a regular, current-uid-owned, `0600`
/// file and not a symlink before its contents are trusted.
pub(crate) fn read_file(path: &Path) -> io::Result<String> {
    verify_token_file(path)?;
    fs::read_to_string(path).map(|value| value.trim().to_owned())
}

fn token_tmp_path(parent: &Path) -> io::Result<PathBuf> {
    let mut suffix = [0_u8; 8];
    getrandom::fill(&mut suffix)
        .map_err(|error| io::Error::other(format!("token temp name generation failed: {error}")))?;
    Ok(parent.join(format!(".animathio-token-{}.tmp", hex(&suffix))))
}

fn verify_token_file(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "token file is a symlink",
        ));
    }
    if !meta.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "token file is not a regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let uid = rustix::process::getuid().as_raw();
        if meta.uid() != uid {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "token file is not owned by the current user",
            ));
        }
        if meta.mode() & 0o077 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "token file permissions allow group or other access",
            ));
        }
    }
    Ok(())
}

/// Compare a provided token against the expected one in constant time.
pub(crate) fn matches(provided: Option<&str>, expected: &str) -> bool {
    let Some(provided) = provided else {
        return false;
    };
    let provided = provided.as_bytes();
    let expected = expected.as_bytes();
    provided.len() == expected.len() && bool::from(provided.ct_eq(expected))
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_tokens_are_unique_and_hex() {
        let a = generate().unwrap();
        let b = generate().unwrap();
        assert_eq!(a.len(), TOKEN_BYTES * 2);
        assert!(a.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn comparison_is_exact_and_case_sensitive() {
        let token = generate().unwrap();
        assert!(matches(Some(&token), &token));
        assert!(!matches(None, &token));
        assert!(!matches(Some(""), &token));
        assert!(!matches(Some(&token.to_uppercase()), &token));
        assert!(!matches(Some(&token[..token.len() - 2]), &token));
    }

    #[cfg(unix)]
    #[test]
    fn written_token_file_is_private() {
        use std::os::unix::fs::MetadataExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("control.token");
        write_file(&path, "secret").unwrap();
        let mode = fs::symlink_metadata(&path).unwrap().mode();
        assert_eq!(mode & 0o777, 0o600);
        assert_eq!(read_file(&path).unwrap(), "secret");
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_token_path_is_not_followed_on_write() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        fs::write(&target, b"original").unwrap();
        let link = dir.path().join("control.token");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        write_file(&link, "replacement").unwrap();

        // The symlink itself is replaced, not the file it pointed at.
        assert!(
            !fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "original");
        assert_eq!(read_file(&link).unwrap(), "replacement");
    }

    #[cfg(unix)]
    #[test]
    fn foreign_mode_token_is_rejected_on_read() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("control.token");
        write_file(&path, "secret").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_file(&path).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_token_is_rejected_on_read() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        write_file(&target, "secret").unwrap();
        let link = dir.path().join("control.token");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(read_file(&link).is_err());
    }
}
