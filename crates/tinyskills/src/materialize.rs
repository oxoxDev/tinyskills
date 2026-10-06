//! Rebuilding a skill tree on disk from a host's resolved skill set.
//!
//! A host that resolves which skills an agent has (from bundles on disk,
//! stored documents, or both) writes the result into a `<root>/<dir>/` tree
//! that discovery and the resource readers then scan. [`materialize_tree`]
//! rebuilds that tree from nothing on every call, so a skill dropped from the
//! set disappears from disk.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::catalog::is_safe_segment;
use crate::model::SKILL_MD;

/// The deepest directory nesting [`materialize_tree`] copies below one
/// entry's directory.
pub const MAX_MATERIALIZE_DEPTH: usize = 32;

/// Where one materialized skill's content comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MaterializeSource {
    /// A bundle directory, copied recursively: regular files and directories
    /// only, symlinks skipped.
    Dir(PathBuf),
    /// One `SKILL.md` document, written as the only file.
    Document(String),
}

/// One skill to write under the tree's root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterializeEntry {
    /// The directory name under the root; must pass
    /// [`is_safe_segment`](crate::is_safe_segment).
    pub dir_name: String,
    /// The skill's content.
    pub source: MaterializeSource,
}

/// What [`materialize_tree`] wrote.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MaterializeReport {
    /// Each entry's directory name, in the order written.
    pub dirs: Vec<String>,
    /// Regular files written or copied.
    pub files: usize,
    /// Symlinks found in source directories and not followed.
    pub skipped_symlinks: usize,
}

/// Why [`materialize_tree`] failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MaterializeError {
    /// An entry's directory name is not a single safe path segment.
    #[error("`{dir_name}` is not a safe skill directory name")]
    UnsafeDirName {
        /// The refused name.
        dir_name: String,
    },
    /// Two entries share a directory name.
    #[error("two skills would both be written to `{dir_name}`")]
    DuplicateDirName {
        /// The shared name.
        dir_name: String,
    },
    /// A source directory nests deeper than [`MAX_MATERIALIZE_DEPTH`].
    #[error("{path} nests more than {max} directories deep")]
    TooDeep {
        /// The first directory past the limit.
        path: PathBuf,
        /// The limit.
        max: usize,
    },
    /// A filesystem operation failed.
    #[error("{context} {path}: {source}")]
    Io {
        /// The operation that failed.
        context: &'static str,
        /// The path involved.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
}

/// Replaces everything under `root` with one directory per entry.
///
/// Every `dir_name` is checked before anything on disk changes. Then `root`
/// is removed (a symlink at `root` is removed, not followed) and recreated,
/// and each entry is written to `root/<dir_name>/`: a
/// [`MaterializeSource::Document`] as its `SKILL.md`, a
/// [`MaterializeSource::Dir`] as a recursive copy that skips symlinks and
/// other non-regular files.
///
/// # Errors
///
/// [`MaterializeError::UnsafeDirName`] or
/// [`MaterializeError::DuplicateDirName`] before anything is touched;
/// [`MaterializeError::TooDeep`] when a source nests past
/// [`MAX_MATERIALIZE_DEPTH`]; [`MaterializeError::Io`] when clearing, creating,
/// reading, or writing fails. A failure part-way leaves the tree partially
/// written; the next call rebuilds it.
pub fn materialize_tree(
    root: &Path,
    entries: &[MaterializeEntry],
) -> Result<MaterializeReport, MaterializeError> {
    let mut seen = BTreeSet::new();
    for entry in entries {
        if !is_safe_segment(&entry.dir_name) {
            return Err(MaterializeError::UnsafeDirName {
                dir_name: entry.dir_name.clone(),
            });
        }
        if !seen.insert(entry.dir_name.as_str()) {
            return Err(MaterializeError::DuplicateDirName {
                dir_name: entry.dir_name.clone(),
            });
        }
    }

    clear(root)?;
    at("creating skill tree", root, std::fs::create_dir_all(root))?;

    let mut report = MaterializeReport::default();
    for entry in entries {
        let dest = root.join(&entry.dir_name);
        match &entry.source {
            MaterializeSource::Dir(src) => copy_dir(src, &dest, 0, &mut report)?,
            MaterializeSource::Document(document) => {
                at("creating skill dir", &dest, std::fs::create_dir_all(&dest))?;
                let file = dest.join(SKILL_MD);
                at("writing", &file, std::fs::write(&file, document))?;
                report.files += 1;
            }
        }
        report.dirs.push(entry.dir_name.clone());
    }
    Ok(report)
}

fn clear(root: &Path) -> Result<(), MaterializeError> {
    match std::fs::symlink_metadata(root) {
        Ok(metadata) if metadata.is_dir() => {
            at("clearing skill tree", root, std::fs::remove_dir_all(root))
        }
        Ok(_) => at("clearing skill tree", root, std::fs::remove_file(root)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io("reading skill tree", root, error)),
    }
}

fn copy_dir(
    src: &Path,
    dest: &Path,
    depth: usize,
    report: &mut MaterializeReport,
) -> Result<(), MaterializeError> {
    if depth > MAX_MATERIALIZE_DEPTH {
        return Err(MaterializeError::TooDeep {
            path: src.to_path_buf(),
            max: MAX_MATERIALIZE_DEPTH,
        });
    }
    at("creating", dest, std::fs::create_dir_all(dest))?;
    let entries = at("reading", src, std::fs::read_dir(src))?;
    for entry in entries {
        let entry = at("reading", src, entry)?;
        let from = entry.path();
        let file_type = at("reading", &from, entry.file_type())?;
        let to = dest.join(entry.file_name());
        if file_type.is_symlink() {
            report.skipped_symlinks += 1;
        } else if file_type.is_dir() {
            copy_dir(&from, &to, depth + 1, report)?;
        } else if file_type.is_file() {
            at("copying", &from, std::fs::copy(&from, &to))?;
            report.files += 1;
        }
    }
    Ok(())
}

fn at<T>(
    context: &'static str,
    path: &Path,
    result: std::io::Result<T>,
) -> Result<T, MaterializeError> {
    result.map_err(|source| io(context, path, source))
}

fn io(context: &'static str, path: &Path, source: std::io::Error) -> MaterializeError {
    MaterializeError::Io {
        context,
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(test)]
#[path = "materialize_tests.rs"]
mod tests;
