//! Rebuilding a skill tree on disk from a host's resolved skill set.
//!
//! A host that resolves which skills an agent has (from bundles on disk,
//! stored documents, or both) writes the result into a `<root>/<dir>/` tree
//! that discovery and the resource readers then scan. [`materialize_tree`]
//! rebuilds that tree from nothing on every call, so a skill dropped from the
//! set disappears from disk.

use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::ambient_authority;
use cap_std::fs::{Dir, File, OpenOptions};

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
    /// A source directory is the destination root, inside it, or contains it.
    #[error("{source_dir} overlaps the destination {root}")]
    OverlappingTrees {
        /// The destination root.
        root: PathBuf,
        /// The overlapping source directory.
        source_dir: PathBuf,
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
/// Source files larger than [`MAX_MATERIALIZE_FILE_BYTES`] are refused.
///
/// The ancestors of `root` and of each source directory are resolved once
/// with [`std::fs::canonicalize`] (the nearest existing ancestor, for a
/// destination that does not exist yet), then walked component by component
/// through directory handles that never follow a symlink; missing destination
/// ancestors are created the same way. Below that point every source and
/// destination operation is made relative to an open handle and never follows
/// a symlink: a source entry swapped for a symlink while the copy runs fails
/// the copy, and replacing an ancestor, `root`, or any destination directory
/// with a symlink after the canonicalization fails the call rather than
/// writing through it.
///
/// Not covered: the canonicalization itself, and an ancestor directory being
/// renamed to a different real directory after it.
///
/// # Errors
///
/// [`MaterializeError::UnsafeDirName`] or
/// [`MaterializeError::DuplicateDirName`] before anything is touched;
/// [`MaterializeError::OverlappingTrees`] when a source directory is `root`,
/// inside it, or contains it; [`MaterializeError::SymlinkedSource`] when a
/// source directory is a symlink; [`MaterializeError::FileTooLarge`] when a source file exceeds the
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

    reject_overlap(root, entries)?;
    let tree = DestDir::replace_root(root)?;

    let mut report = MaterializeReport::default();
    for entry in entries {
        let dest = root.join(&entry.dir_name);
        match &entry.source {
            MaterializeSource::Dir(src) => {
                let source = SourceDir::open_root(src)?;
                let out = at(
                    "creating",
                    &dest,
                    tree.create_dir(OsStr::new(&entry.dir_name)),
                )?;
                copy_dir(&source, src, &out, &dest, 0, &mut report)?;
            }
            MaterializeSource::Document(document) => {
                let out = at(
                    "creating skill dir",
                    &dest,
                    tree.create_dir(OsStr::new(&entry.dir_name)),
                )?;
                let file = dest.join(SKILL_MD);
                let mut handle = at("creating", &file, out.create_file(OsStr::new(SKILL_MD)))?;
                at("writing", &file, handle.write_all(document.as_bytes()))?;
                report.files += 1;
            }
        }
        report.dirs.push(entry.dir_name.clone());
    }
    Ok(report)
}

fn reject_overlap(root: &Path, entries: &[MaterializeEntry]) -> Result<(), MaterializeError> {
    let mut sources = entries
        .iter()
        .filter_map(|entry| match &entry.source {
            MaterializeSource::Dir(src) => Some(src),
            MaterializeSource::Document(_) => None,
        })
        .peekable();
    if sources.peek().is_none() {
        return Ok(());
    }
    let target = canonical_target(root);
    for src in sources {
        let Ok(resolved) = std::fs::canonicalize(src) else {
            continue;
        };
        if resolved.starts_with(&target) || target.starts_with(&resolved) {
            return Err(MaterializeError::OverlappingTrees {
                root: root.to_path_buf(),
                source_dir: src.clone(),
            });
        }
    }
    Ok(())
}

fn canonical_target(root: &Path) -> PathBuf {
    let mut missing = Vec::new();
    let mut existing = root;
    while let Some((parent, name)) = parent_and_name(existing) {
        if let Ok(base) = std::fs::canonicalize(existing) {
            return missing
                .iter()
                .rev()
                .fold(base, |path, name| path.join(name));
        }
        missing.push(name);
        existing = parent;
    }
    root.to_path_buf()
}

fn copy_dir(
    source: &SourceDir,
    src: &Path,
    out: &DestDir,
    dest: &Path,
    depth: usize,
    report: &mut MaterializeReport,
) -> Result<(), MaterializeError> {
    for entry in at("reading", src, source.entries())? {
        let (name, kind) = at("reading", src, entry)?;
        let from = src.join(&name);
        let to = dest.join(&name);
        match kind {
            EntryKind::Symlink => report.skipped_symlinks += 1,
            EntryKind::Dir => {
                if depth >= MAX_MATERIALIZE_DEPTH {
                    return Err(MaterializeError::TooDeep {
                        path: from,
                        max: MAX_MATERIALIZE_DEPTH,
                    });
                }
                let child = at("reading", &from, source.open_dir(&name))?;
                let child_out = at("creating", &to, out.create_dir(&name))?;
                copy_dir(&child, &from, &child_out, &to, depth + 1, report)?;
            }
            EntryKind::File => {
                if copy_file(source, &name, &from, out, &to)? {
                    report.files += 1;
                }
            }
            EntryKind::Other => {}
        }
    }
    Ok(())
}

fn copy_file(
    source: &SourceDir,
    name: &OsStr,
    from: &Path,
    out: &DestDir,
    to: &Path,
) -> Result<bool, MaterializeError> {
    let file = at("copying", from, source.open_file(name))?;
    let metadata = at("copying", from, file.metadata())?;
    if !metadata.is_file() {
        return Ok(false);
    }
    let too_large = || MaterializeError::FileTooLarge {
        path: from.to_path_buf(),
        max: MAX_MATERIALIZE_FILE_BYTES,
    };
    if metadata.len() > MAX_MATERIALIZE_FILE_BYTES {
        return Err(too_large());
    }
    let mut handle = at("creating", to, out.create_file(name))?;
    let copied = at(
        "copying",
        from,
        std::io::copy(&mut file.take(MAX_MATERIALIZE_FILE_BYTES + 1), &mut handle),
    )?;
    if copied > MAX_MATERIALIZE_FILE_BYTES {
        return Err(too_large());
    }
    Ok(true)
}

enum EntryKind {
    Dir,
    File,
    Symlink,
    Other,
}

struct SourceDir(Dir);

impl SourceDir {
    fn open_root(path: &Path) -> Result<Self, MaterializeError> {
        let metadata = at("reading", path, std::fs::symlink_metadata(path))?;
        if metadata.file_type().is_symlink() {
            return Err(MaterializeError::SymlinkedSource {
                path: path.to_path_buf(),
            });
        }
        Ok(Self(at("reading", path, open_dir_nofollow_at(path))?))
    }

    fn open_dir(&self, name: &OsStr) -> std::io::Result<Self> {
        Ok(Self(self.0.open_dir_nofollow(name)?))
    }

    fn open_file(&self, name: &OsStr) -> std::io::Result<File> {
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        self.0.open_with(name, &options)
    }

    fn entries(
        &self,
    ) -> std::io::Result<impl Iterator<Item = std::io::Result<(OsString, EntryKind)>>> {
        Ok(self.0.entries()?.map(|entry| {
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
            Ok((entry.file_name(), kind))
        }))
    }
}

struct DestDir(Dir);

impl DestDir {
    fn replace_root(root: &Path) -> Result<Self, MaterializeError> {
        let (parent_path, name) =
            parent_and_name(root).ok_or_else(|| io("creating skill tree", root, invalid_root()))?;
        let parent = at(
            "creating skill tree",
            parent_path,
            open_anchored(parent_path, true),
        )?;
        match parent.symlink_metadata(name) {
            Ok(metadata) if metadata.is_dir() => {
                at("clearing skill tree", root, parent.remove_dir_all(name))?;
            }
            Ok(_) => at(
                "clearing skill tree",
                root,
                parent.remove_file_or_symlink(name),
            )?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io("reading skill tree", root, error)),
        }
        at("creating skill tree", root, parent.create_dir(name))?;
        let dir = at("creating skill tree", root, parent.open_dir_nofollow(name))?;
        Ok(Self(dir))
    }

    fn create_dir(&self, name: &OsStr) -> std::io::Result<Self> {
        self.0.create_dir(name)?;
        Ok(Self(self.0.open_dir_nofollow(name)?))
    }

    fn create_file(&self, name: &OsStr) -> std::io::Result<File> {
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        self.0.open_with(name, &options)
    }
}

fn parent_and_name(path: &Path) -> Option<(&Path, &OsStr)> {
    let name = path.file_name()?;
    let parent = path.parent()?;
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    Some((parent, name))
}

fn invalid_root() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "path has no final component",
    )
}

fn open_dir_nofollow_at(path: &Path) -> std::io::Result<Dir> {
    let (parent, name) = parent_and_name(path).ok_or_else(invalid_root)?;
    open_anchored(parent, false)?.open_dir_nofollow(name)
}

fn open_anchored(path: &Path, create: bool) -> std::io::Result<Dir> {
    let mut missing = Vec::new();
    let mut existing = path.to_path_buf();
    let base = loop {
        match std::fs::canonicalize(&existing) {
            Ok(base) => break base,
            Err(error) if create && error.kind() == std::io::ErrorKind::NotFound => {
                let (parent, name) = parent_and_name(&existing).ok_or_else(invalid_root)?;
                if !matches!(
                    Path::new(name).components().next(),
                    Some(Component::Normal(_))
                ) {
                    return Err(invalid_root());
                }
                missing.push(name.to_owned());
                existing = parent.to_path_buf();
            }
            Err(error) => return Err(error),
        }
    };
    let mut dir = open_chain(&base)?;
    for name in missing.iter().rev() {
        match dir.create_dir(name) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        dir = dir.open_dir_nofollow(name)?;
    }
    Ok(dir)
}

fn open_chain(path: &Path) -> std::io::Result<Dir> {
    let mut head = PathBuf::new();
    let mut components = path.components().peekable();
    while let Some(component) = components.next_if(|component| {
        matches!(
            component,
            Component::Prefix(_) | Component::RootDir | Component::CurDir
        )
    }) {
        head.push(component);
    }
    if head.as_os_str().is_empty() {
        head.push(".");
    }
    let mut dir = Dir::open_ambient_dir(&head, ambient_authority())?;
    for component in components {
        let Component::Normal(name) = component else {
            return Err(invalid_root());
        };
        dir = dir.open_dir_nofollow(name)?;
    }
    Ok(dir)
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
