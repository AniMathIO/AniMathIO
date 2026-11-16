//! Filesystem path validation shared by every method that takes a path.
//!
//! All input paths are canonicalized and must resolve to a regular file;
//! output paths canonicalize their nearest existing ancestor and refuse a
//! non-directory ancestor or an existing non-file target. Symlinks in that
//! ancestor are resolved before use so a crafted link cannot point the control
//! surface at an unexpected location. Missing parent directories are allowed:
//! the export writer creates them.

use crate::ControlError;
use std::ffi::OsString;
use std::io::ErrorKind;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Validate an existing regular file, returning its canonical path.
pub fn validate_existing_file(path: &Path) -> Result<PathBuf, ControlError> {
    let canonical = path
        .canonicalize()
        .map_err(|error| ControlError::InvalidPath(format!("{}: {error}", path.display())))?;
    if !canonical.is_file() {
        return Err(ControlError::InvalidPath(format!(
            "not a regular file: {}",
            path.display()
        )));
    }
    Ok(canonical)
}

/// Read a whole file without exceeding `limit` bytes.
pub fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, ControlError> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(ControlError::InvalidPath(format!(
            "file exceeds the {limit} byte limit: {}",
            path.display()
        )));
    }
    Ok(bytes)
}

/// Validate an output path: canonicalize its nearest existing ancestor, which
/// must be a directory, and allow the remaining directories to be missing (the
/// writer creates them). An existing target must be a regular file, not a
/// directory or special file.
pub fn validate_output_file(path: &Path) -> Result<PathBuf, ControlError> {
    let file_name = path.file_name().ok_or_else(|| {
        ControlError::InvalidPath(format!("path has no file name: {}", path.display()))
    })?;
    let mut ancestor = match path.parent().filter(|p| !p.as_os_str().is_empty()) {
        Some(parent) => parent.to_path_buf(),
        None => current_dir()?,
    };
    let mut missing: Vec<OsString> = Vec::new();
    let canonical = loop {
        match ancestor.canonicalize() {
            Ok(canonical) => break canonical,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let Some(name) = ancestor.file_name() else {
                    // A relative path whose parents are all missing: anchor at
                    // the current directory, which canonicalizes below.
                    break current_dir()?;
                };
                missing.push(OsString::from(name));
                ancestor = match ancestor.parent() {
                    Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
                    _ => current_dir()?,
                };
            }
            Err(error) => {
                return Err(ControlError::InvalidPath(format!(
                    "{}: {error}",
                    path.display()
                )));
            }
        }
    };
    if !canonical.is_dir() {
        return Err(ControlError::InvalidPath(format!(
            "parent is not a directory: {}",
            path.display()
        )));
    }
    let mut target = canonical;
    for component in missing.iter().rev() {
        target.push(component);
    }
    target.push(file_name);
    if target.exists() && !target.is_file() {
        return Err(ControlError::InvalidPath(format!(
            "not a regular file: {}",
            path.display()
        )));
    }
    Ok(target)
}

/// Best-effort canonical form of `path`: the file itself when it exists,
/// otherwise the canonicalized nearest existing ancestor with the missing
/// tail re-attached (the same rule [`validate_output_file`] applies), so a
/// not-yet-written output still resolves to the file it would become. Falls
/// back to the given form when nothing can be resolved.
fn resolve(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    let Some(file_name) = path.file_name() else {
        return path.to_path_buf();
    };
    let fallback = || path.to_path_buf();
    let mut ancestor = match path.parent().filter(|p| !p.as_os_str().is_empty()) {
        Some(parent) => parent.to_path_buf(),
        // A bare file name relative to the current directory.
        None => {
            return match current_dir() {
                Ok(dir) => dir.join(file_name),
                Err(_) => fallback(),
            };
        }
    };
    let mut missing: Vec<OsString> = Vec::new();
    let mut base = loop {
        match ancestor.canonicalize() {
            Ok(canonical) => break canonical,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let Some(name) = ancestor.file_name() else {
                    // A relative path whose parents are all missing: anchor at
                    // the current directory, which canonicalizes below.
                    break match current_dir() {
                        Ok(dir) => dir,
                        Err(_) => return fallback(),
                    };
                };
                missing.push(OsString::from(name));
                ancestor = match ancestor.parent() {
                    Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
                    _ => {
                        break match current_dir() {
                            Ok(dir) => dir,
                            Err(_) => return fallback(),
                        };
                    }
                };
            }
            Err(_) => return fallback(),
        }
    };
    for component in missing.iter().rev() {
        base.push(component);
    }
    base.push(file_name);
    base
}

/// Whether two paths name the same file. Both are resolved with [`resolve`],
/// so a not-yet-existing output compares against the file it would become,
/// including through symlinked parents. Comparison is case-insensitive on
/// Windows and macOS, where the default filesystems ignore case.
#[must_use]
pub fn same_file(a: &Path, b: &Path) -> bool {
    let (a, b) = (resolve(a), resolve(b));
    if cfg!(any(windows, target_os = "macos")) {
        a.as_os_str().eq_ignore_ascii_case(b.as_os_str())
    } else {
        a == b
    }
}

/// Refuse an export output that names the open project file: exporting an
/// MP4 over `project.amio` would destroy the project. Every surface checks
/// this before writing — the editor Export dialog, the editor's and the
/// headless control/MCP `export` method, and `animathio-cli export`. An
/// output that merely lives next to the project (or nowhere yet) is fine.
pub fn ensure_distinct_from_project(output: &Path, project: &Path) -> Result<(), ControlError> {
    if same_file(output, project) {
        return Err(ControlError::InvalidPath(
            "the output path is the open project file; choose a different file name".to_owned(),
        ));
    }
    Ok(())
}

fn current_dir() -> Result<PathBuf, ControlError> {
    std::env::current_dir()
        .map_err(|error| ControlError::InvalidPath(format!("current directory: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_file_is_canonicalized() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(
            validate_existing_file(&file).unwrap(),
            file.canonicalize().unwrap()
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_resolved() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, b"x").unwrap();
        let link = dir.path().join("link.txt");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert_eq!(
            validate_existing_file(&link).unwrap(),
            file.canonicalize().unwrap()
        );
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

    #[test]
    fn missing_output_parents_are_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let out = validate_output_file(&dir.path().join("a/b/c/out.mp4")).unwrap();
        assert_eq!(
            out,
            dir.path().canonicalize().unwrap().join("a/b/c/out.mp4")
        );
        // A file in the middle of the path is not a directory ancestor.
        let file = dir.path().join("plain.txt");
        std::fs::write(&file, b"x").unwrap();
        assert!(validate_output_file(&file.join("deeper/out.mp4")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_output_parents_are_resolved() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let out = validate_output_file(&link.join("nested/out.mp4")).unwrap();
        assert_eq!(
            out,
            real.canonicalize().unwrap().join("nested/out.mp4"),
            "the resolved ancestor is used, not the link"
        );
    }

    #[test]
    fn the_project_file_is_the_same_file_under_any_spelling() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project.amio");
        std::fs::write(&project, b"x").unwrap();
        assert!(same_file(&project, &project));
        assert!(same_file(&project, &dir.path().join("./project.amio")));
        // `..` through an existing directory still names the project.
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        assert!(same_file(&project, &dir.path().join("sub/../project.amio")));
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_resolved_in_both_directions() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let project = real.join("project.amio");
        std::fs::write(&project, b"x").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(same_file(&link.join("project.amio"), &project));
        // A not-yet-written output through a symlinked parent resolves to the
        // real directory, so it only matches when the file names agree.
        assert!(same_file(&link.join("out.mp4"), &real.join("out.mp4")));
        assert!(!same_file(&link.join("out.mp4"), &project));
    }

    #[test]
    fn other_files_in_the_project_directory_are_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project.amio");
        std::fs::write(&project, b"x").unwrap();
        let other = dir.path().join("out.mp4");
        std::fs::write(&other, b"y").unwrap();
        assert!(!same_file(&other, &project));
        assert!(ensure_distinct_from_project(&other, &project).is_ok());
        // Outputs that do not exist yet compare by the file they would become.
        assert!(ensure_distinct_from_project(&dir.path().join("new.mp4"), &project).is_ok());
        assert!(
            ensure_distinct_from_project(&dir.path().join("nested/deeper/new.mp4"), &project)
                .is_ok()
        );
    }

    #[test]
    fn exporting_onto_the_project_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project.amio");
        std::fs::write(&project, b"x").unwrap();
        for spelling in [project.clone(), dir.path().join("./project.amio")] {
            let error = ensure_distinct_from_project(&spelling, &project).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("the output path is the open project file"),
                "{error}"
            );
            assert!(error.to_string().contains("choose a different file name"));
        }
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn comparison_ignores_case_where_the_filesystem_does() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project.amio");
        std::fs::write(&project, b"x").unwrap();
        assert!(same_file(&dir.path().join("PROJECT.AMIO"), &project));
        assert!(ensure_distinct_from_project(&dir.path().join("Project.Amio"), &project).is_err());
    }
}
