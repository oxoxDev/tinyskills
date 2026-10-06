//! Rebuilding a skill tree on disk from a host's resolved skill set.
//!
//! A host that resolves which skills an agent has (from bundles on disk,
//! stored documents, or both) writes the result into a `<root>/<dir>/` tree
//! that discovery and the resource readers then scan. [`materialize_tree`]
//! rebuilds that tree from nothing on every call, so a skill dropped from the
//! set disappears from disk.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::catalog::is_safe_segment;
use crate::model::SKILL_MD;

/// The deepest directory nesting [`materialize_tree`] copies below one
/// entry's directory.
pub const MAX_MATERIALIZE_DEPTH: usize = 32;

/// The largest single file [`materialize_tree`] copies out of a bundle
/// directory.
pub const MAX_MATERIALIZE_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// Where one materialized skill's content comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MaterializeSource {
    /// A bundle directory, copied recursively: regular files and directories
    /// only, symlinks skipped. The directory itself must not be a symlink.
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
    /// A source directory is itself a symlink.
    #[error("{path} is a symlink; a bundle directory must be a real directory")]
    SymlinkedSource {
        /// The refused source directory.
        path: PathBuf,
    },
    /// A source file is larger than [`MAX_MATERIALIZE_FILE_BYTES`].
    #[error("{path} is larger than {max} bytes")]
    FileTooLarge {
        /// The oversized file.
        path: PathBuf,
        /// The limit.
        max: u64,
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
/// Source files larger than [`MAX_MATERIALIZE_FILE_BYTES`] are refused. On
/// Unix each source entry is opened relative to its parent directory without
/// following symlinks, so replacing an entry with a symlink while the copy
/// runs fails the copy rather than reading through the link. Destination
/// entries are created exclusively and never follow a symlink in the final
/// path component. Neither guarantee covers the ancestors of `root` or of a
/// source directory, nor a non-Unix platform: callers must own the
/// destination's parent directory and must not copy a source tree an
/// untrusted party can modify concurrently.
///
/// # Errors
///
/// [`MaterializeError::UnsafeDirName`] or
/// [`MaterializeError::DuplicateDirName`] before anything is touched;
/// [`MaterializeError::SymlinkedSource`] when a source directory is a
/// symlink; [`MaterializeError::FileTooLarge`] when a source file exceeds the
/// limit; [`MaterializeError::TooDeep`] when a source nests past
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
            MaterializeSource::Dir(src) => {
                let source = SourceDir::open_root(src)?;
                copy_dir(&source, src, &dest, 0, &mut report)?;
            }
            MaterializeSource::Document(document) => {
                at("creating skill dir", &dest, std::fs::create_dir(&dest))?;
                let file = dest.join(SKILL_MD);
                let mut out = create_file(&file)?;
                at("writing", &file, out.write_all(document.as_bytes()))?;
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
    source: &SourceDir,
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
    at("creating", dest, std::fs::create_dir(dest))?;
    for (name, kind) in at("reading", src, source.entries())? {
        let from = src.join(&name);
        let to = dest.join(&name);
        match kind {
            EntryKind::Symlink => report.skipped_symlinks += 1,
            EntryKind::Dir => {
                let child = at("reading", &from, source.open_dir(&name))?;
                copy_dir(&child, &from, &to, depth + 1, report)?;
            }
            EntryKind::File => {
                copy_file(source, &name, &from, &to)?;
                report.files += 1;
            }
            EntryKind::Other => {}
        }
    }
    Ok(())
}

fn copy_file(
    source: &SourceDir,
    name: &OsString,
    from: &Path,
    to: &Path,
) -> Result<(), MaterializeError> {
    let file = at("copying", from, source.open_file(name))?;
    let metadata = at("copying", from, file.metadata())?;
    if !metadata.is_file() {
        return Ok(());
    }
    let too_large = || MaterializeError::FileTooLarge {
        path: from.to_path_buf(),
        max: MAX_MATERIALIZE_FILE_BYTES,
    };
    if metadata.len() > MAX_MATERIALIZE_FILE_BYTES {
        return Err(too_large());
    }
    let mut out = create_file(to)?;
    let copied = at(
        "copying",
        from,
        std::io::copy(&mut file.take(MAX_MATERIALIZE_FILE_BYTES + 1), &mut out),
    )?;
    if copied > MAX_MATERIALIZE_FILE_BYTES {
        return Err(too_large());
    }
    Ok(())
}

fn create_file(path: &Path) -> Result<File, MaterializeError> {
    at(
        "creating",
        path,
        OpenOptions::new().write(true).create_new(true).open(path),
    )
}

enum EntryKind {
    Dir,
    File,
    Symlink,
    Other,
}

#[cfg(unix)]
struct SourceDir(File);

#[cfg(unix)]
impl SourceDir {
    fn open_root(path: &Path) -> Result<Self, MaterializeError> {
        let metadata = at("reading", path, std::fs::symlink_metadata(path))?;
        if metadata.file_type().is_symlink() {
            return Err(MaterializeError::SymlinkedSource {
                path: path.to_path_buf(),
            });
        }
        let fd = at("reading", path, open_nofollow_dir(rustix::fs::CWD, path))?;
        Ok(Self(File::from(fd)))
    }

    fn open_dir(&self, name: &OsString) -> std::io::Result<Self> {
        let fd = open_nofollow_dir(&self.0, name)?;
        Ok(Self(File::from(fd)))
    }

    fn open_file(&self, name: &OsString) -> std::io::Result<File> {
        use rustix::fs::{Mode, OFlags, openat};
        let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK;
        Ok(File::from(openat(&self.0, name, flags, Mode::empty())?))
    }

    fn entries(&self) -> std::io::Result<Vec<(OsString, EntryKind)>> {
        use rustix::fs::{AtFlags, Dir, FileType, statat};
        use std::os::unix::ffi::OsStringExt;
        let mut found = Vec::new();
        for entry in Dir::read_from(&self.0)? {
            let entry = entry?;
            let raw = entry.file_name().to_bytes();
            if raw == b"." || raw == b".." {
                continue;
            }
            let name = OsString::from_vec(raw.to_vec());
            let file_type = match entry.file_type() {
                FileType::Unknown => {
                    let stat = statat(&self.0, &name, AtFlags::SYMLINK_NOFOLLOW)?;
                    FileType::from_raw_mode(stat.st_mode as _)
                }
                known => known,
            };
            let kind = match file_type {
                FileType::Symlink => EntryKind::Symlink,
                FileType::Directory => EntryKind::Dir,
                FileType::RegularFile => EntryKind::File,
                _ => EntryKind::Other,
            };
            found.push((name, kind));
        }
        Ok(found)
    }
}

#[cfg(unix)]
fn open_nofollow_dir<Fd: std::os::fd::AsFd, P: rustix::path::Arg>(
    parent: Fd,
    name: P,
) -> std::io::Result<std::os::fd::OwnedFd> {
    use rustix::fs::{Mode, OFlags, openat};
    let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::DIRECTORY | OFlags::CLOEXEC;
    Ok(openat(parent, name, flags, Mode::empty())?)
}

#[cfg(not(unix))]
struct SourceDir(PathBuf);

#[cfg(not(unix))]
impl SourceDir {
    fn open_root(path: &Path) -> Result<Self, MaterializeError> {
        let metadata = at("reading", path, std::fs::symlink_metadata(path))?;
        if metadata.file_type().is_symlink() {
            return Err(MaterializeError::SymlinkedSource {
                path: path.to_path_buf(),
            });
        }
        at("reading", path, std::fs::read_dir(path))?;
        Ok(Self(path.to_path_buf()))
    }

    fn open_dir(&self, name: &OsString) -> std::io::Result<Self> {
        let path = self.0.join(name);
        if std::fs::symlink_metadata(&path)?.file_type().is_symlink() {
            return Err(std::io::Error::other("directory became a symlink"));
        }
        Ok(Self(path))
    }

    fn open_file(&self, name: &OsString) -> std::io::Result<File> {
        let path = self.0.join(name);
        if std::fs::symlink_metadata(&path)?.file_type().is_symlink() {
            return Err(std::io::Error::other("file became a symlink"));
        }
        File::open(path)
    }

    fn entries(&self) -> std::io::Result<Vec<(OsString, EntryKind)>> {
        let mut found = Vec::new();
        for entry in std::fs::read_dir(&self.0)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let kind = if file_type.is_symlink() {
                EntryKind::Symlink
            } else if file_type.is_dir() {
                EntryKind::Dir
            } else if file_type.is_file() {
                EntryKind::File
            } else {
                EntryKind::Other
            };
            found.push((entry.file_name(), kind));
        }
        Ok(found)
    }
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
