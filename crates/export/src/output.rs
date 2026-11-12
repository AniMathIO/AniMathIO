//! Non-clobbering, atomic output files.
//!
//! Export must not require the destination's parent directory to exist, must
//! never overwrite silently and must never leave a partial file behind. Bytes
//! therefore go into a temporary file created in the destination directory and
//! are renamed over the destination only once the export finished; a dropped
//! [`AtomicOutput`] removes its temporary file.

use std::{
    fs,
    io::{ErrorKind, Write as _},
    path::{Path, PathBuf},
};

use tempfile::NamedTempFile;

use crate::Error;

/// A destination file written through a sibling temporary file.
///
/// [`AtomicOutput::create`] creates missing parent directories, refuses an
/// existing destination unless overwriting was requested, and only then
/// creates the temporary file. [`AtomicOutput::commit`] renames it into place,
/// so an existing file keeps its contents until a successful export replaces
/// it. Dropping an uncommitted value removes the temporary file.
pub struct AtomicOutput {
    target: PathBuf,
    temp: NamedTempFile,
}

impl AtomicOutput {
    /// Prepare `target` for writing, creating missing parent directories.
    ///
    /// Returns [`Error::AlreadyExists`] when `target` already exists and
    /// `overwrite` is unset, and [`Error::Invalid`] when it is a directory.
    pub fn create(target: &Path, overwrite: bool) -> Result<Self, Error> {
        if target.file_name().is_none() {
            return Err(Error::Invalid(format!(
                "output path has no file name: {}",
                target.display()
            )));
        }
        let parent = target
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        match fs::symlink_metadata(target) {
            Ok(metadata) => {
                if metadata.is_dir() {
                    return Err(Error::Invalid(format!(
                        "output path is a directory: {}",
                        target.display()
                    )));
                }
                if !overwrite {
                    return Err(Error::AlreadyExists(target.to_owned()));
                }
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(Error::Io(error)),
        }
        let temp = NamedTempFile::new_in(parent)?;
        Ok(Self {
            target: target.to_owned(),
            temp,
        })
    }

    /// Destination this output replaces on commit.
    #[must_use]
    pub fn target(&self) -> &Path {
        &self.target
    }

    /// Append bytes to the temporary file.
    pub fn write_all(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.temp.as_file_mut().write_all(bytes)?;
        Ok(())
    }

    /// Flush and rename the temporary file over the destination.
    pub fn commit(self) -> Result<PathBuf, Error> {
        self.temp.as_file().sync_all()?;
        // `persist` renames, replacing an existing file atomically on Unix and
        // through `MoveFileEx` on Windows. On failure the temp file is removed
        // when the returned handle is dropped.
        self.temp
            .persist(&self.target)
            .map_err(|error| Error::Io(error.error))?;
        Ok(self.target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(dir: &Path) -> Vec<PathBuf> {
        let mut paths: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        paths.sort();
        paths
    }

    #[test]
    fn missing_parents_are_created_and_commit_renames_into_place() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a/b/out.mp4");
        let mut output = AtomicOutput::create(&target, false).unwrap();
        assert_eq!(output.target(), target);
        output.write_all(b"encoded").unwrap();
        assert!(!target.exists());
        assert_eq!(output.commit().unwrap(), target);
        assert_eq!(fs::read(&target).unwrap(), b"encoded");
        // The temp file was renamed, not copied.
        assert_eq!(entries(&dir.path().join("a/b")), vec![target.clone()]);
    }

    #[test]
    fn existing_target_requires_overwrite_and_stays_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("out.mp4");
        fs::write(&target, b"old").unwrap();
        assert!(matches!(
            AtomicOutput::create(&target, false),
            Err(Error::AlreadyExists(path)) if path == target
        ));
        assert_eq!(fs::read(&target).unwrap(), b"old");
        assert_eq!(entries(dir.path()), vec![target.clone()]);
        let mut output = AtomicOutput::create(&target, true).unwrap();
        output.write_all(b"new").unwrap();
        output.commit().unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert_eq!(entries(dir.path()), vec![target.clone()]);
    }

    #[test]
    fn dropping_an_uncommitted_output_removes_the_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("nested/out.mp4");
        let mut output = AtomicOutput::create(&target, false).unwrap();
        output.write_all(b"partial").unwrap();
        assert_eq!(entries(&dir.path().join("nested")).len(), 1);
        drop(output);
        assert!(!target.exists());
        assert_eq!(entries(&dir.path().join("nested")).len(), 0);
    }

    #[test]
    fn directories_and_nameless_paths_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            AtomicOutput::create(dir.path(), true),
            Err(Error::Invalid(message)) if message.contains("directory")
        ));
        assert!(matches!(
            AtomicOutput::create(Path::new(".."), true),
            Err(Error::Invalid(message)) if message.contains("no file name")
        ));
    }
}
