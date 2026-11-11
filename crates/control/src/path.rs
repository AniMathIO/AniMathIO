//! Filesystem path validation shared by every method that takes a path.
//!
//! All input paths are canonicalized and must resolve to a regular file;
//! output paths canonicalize their parent and refuse to overwrite a
//! directory. Symlinks are resolved before use so a crafted link cannot point
//! the control surface at an unexpected location.

use crate::ControlError;
use std::path::{Path, PathBuf};

/// Validate an existing regular file, returning its canonical path.
pub fn validate_existing_file(path: &Path) -> Result<PathBuf, ControlError> {
    let canonical = path.canonicalize().map_err(|error| {
        ControlError::InvalidPath(format!("{}: {error}", path.display()))
    })?;
    if !canonical.is_file() {
        return Err(ControlError::InvalidPath(format!(
            "not a regular file: {}",
            path.display()
        )));
    }
    Ok(canonical)
}

/// Validate an output path: its parent must exist and any existing target
/// must be a regular file, not a directory or special file.
pub fn validate_output_file(path: &Path) -> Result<PathBuf, ControlError> {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    let canonical_parent = match parent {
        Some(parent) => parent.canonicalize().map_err(|error| {
            ControlError::InvalidPath(format!("{}: {error}", parent.display()))
        })?,
        None => std::env::current_dir().map_err(|error| {
            ControlError::InvalidPath(format!("current directory: {error}"))
        })?,
    };
    let file_name = path.file_name().ok_or_else(|| {
        ControlError::InvalidPath(format!("path has no file name: {}", path.display()))
    })?;
    let target = canonical_parent.join(file_name);
    if target.exists() && !target.is_file() {
        return Err(ControlError::InvalidPath(format!(
            "not a regular file: {}",
            path.display()
        )));
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_file_is_canonicalized() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(validate_existing_file(&file).unwrap(), file.canonicalize().unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_resolved() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, b"x").unwrap();
        let link = dir.path().join("link.txt");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert_eq!(validate_existing_file(&link).unwrap(), file.canonicalize().unwrap());
    }

    #[test]
    fn directories_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        assert!(validate_existing_file(dir.path()).is_err());
    }

    #[test]
    fn output_parent_is_canonicalized_and_directories_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        let out = validate_output_file(&dir.path().join("out.png")).unwrap();
        assert_eq!(out.parent().unwrap(), dir.path().canonicalize().unwrap());
        assert!(validate_output_file(&sub).is_err());
    }
}
