//! Persistence of fetched catalogs, and the clock that ages them.

use std::collections::HashMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use super::contract::{RegistryEntry, Validators};
use super::error::StoreError;
use super::transport::BoxFuture;
use crate::is_safe_segment;

/// A persisted catalog for one registry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct StoredCatalog {
    /// The storage format; a different value is read as absent.
    pub format: u32,
    /// The entries, in catalog order.
    pub entries: Vec<RegistryEntry>,
    /// Unix seconds of the fetch.
    pub fetched_at: u64,
    /// Validators for a conditional refresh.
    pub validators: Validators,
    /// Upstream items the load dropped.
    pub skipped: usize,
}

impl StoredCatalog {
    /// The storage format this crate writes.
    pub const FORMAT: u32 = 1;

    /// A catalog in the current format.
    #[must_use]
    pub fn new(
        entries: Vec<RegistryEntry>,
        fetched_at: u64,
        validators: Validators,
        skipped: usize,
    ) -> Self {
        Self {
            format: Self::FORMAT,
            entries,
            fetched_at,
            validators,
            skipped,
        }
    }
}

impl Default for StoredCatalog {
    fn default() -> Self {
        Self::new(Vec::new(), 0, Validators::default(), 0)
    }
}

/// Where fetched catalogs persist between runs.
pub trait CatalogStore: Send + Sync {
    /// The stored catalog for `registry`, or `None`.
    fn load<'a>(
        &'a self,
        registry: &'a str,
    ) -> BoxFuture<'a, Result<Option<StoredCatalog>, StoreError>>;

    /// Replace the stored catalog for `registry`.
    fn save<'a>(
        &'a self,
        registry: &'a str,
        catalog: &'a StoredCatalog,
    ) -> BoxFuture<'a, Result<(), StoreError>>;
}

impl<T: CatalogStore + ?Sized> CatalogStore for std::sync::Arc<T> {
    fn load<'a>(
        &'a self,
        registry: &'a str,
    ) -> BoxFuture<'a, Result<Option<StoredCatalog>, StoreError>> {
        (**self).load(registry)
    }

    fn save<'a>(
        &'a self,
        registry: &'a str,
        catalog: &'a StoredCatalog,
    ) -> BoxFuture<'a, Result<(), StoreError>> {
        (**self).save(registry, catalog)
    }
}

/// An in-process store; nothing survives the process.
#[derive(Debug, Default)]
pub struct MemoryCatalogStore {
    catalogs: Mutex<HashMap<String, StoredCatalog>>,
}

impl MemoryCatalogStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl CatalogStore for MemoryCatalogStore {
    fn load<'a>(
        &'a self,
        registry: &'a str,
    ) -> BoxFuture<'a, Result<Option<StoredCatalog>, StoreError>> {
        let stored = self
            .catalogs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(registry)
            .cloned();
        Box::pin(async move { Ok(stored) })
    }

    fn save<'a>(
        &'a self,
        registry: &'a str,
        catalog: &'a StoredCatalog,
    ) -> BoxFuture<'a, Result<(), StoreError>> {
        self.catalogs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(registry.to_owned(), catalog.clone());
        Box::pin(async { Ok(()) })
    }
}

/// One JSON file per registry, `<dir>/<registry>.json`, written atomically.
///
/// A file in another format reads as absent. A registry id that is not a
/// plain path segment, a symlinked file and a file over the read limit are
/// refused.
#[derive(Debug, Clone)]
pub struct FileCatalogStore {
    dir: PathBuf,
    max_bytes: u64,
}

impl FileCatalogStore {
    /// Default read limit, 512 MiB.
    pub const DEFAULT_MAX_BYTES: u64 = 512 * 1024 * 1024;

    /// A store in `dir`, created on first save.
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            max_bytes: Self::DEFAULT_MAX_BYTES,
        }
    }

    /// Set the read limit.
    #[must_use]
    pub fn with_max_bytes(mut self, max_bytes: u64) -> Self {
        self.max_bytes = max_bytes;
        self
    }

    fn path(&self, registry: &str) -> Result<PathBuf, StoreError> {
        if !is_safe_segment(registry) {
            return Err(StoreError::InvalidId(registry.to_owned()));
        }
        Ok(self.dir.join(format!("{registry}.json")))
    }
}

impl CatalogStore for FileCatalogStore {
    fn load<'a>(
        &'a self,
        registry: &'a str,
    ) -> BoxFuture<'a, Result<Option<StoredCatalog>, StoreError>> {
        let path = self.path(registry);
        let max_bytes = self.max_bytes;
        Box::pin(async move {
            let path = path?;
            blocking(move || read_catalog(&path, max_bytes)).await
        })
    }

    fn save<'a>(
        &'a self,
        registry: &'a str,
        catalog: &'a StoredCatalog,
    ) -> BoxFuture<'a, Result<(), StoreError>> {
        let path = self.path(registry);
        let dir = self.dir.clone();
        Box::pin(async move {
            let path = path?;
            let bytes =
                serde_json::to_vec(catalog).map_err(|error| StoreError::Io(error.to_string()))?;
            blocking(move || write_catalog(&dir, &path, &bytes)).await
        })
    }
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, StoreError> + Send + 'static,
) -> Result<T, StoreError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| StoreError::Io(error.to_string()))?
}

#[derive(Deserialize)]
struct FormatProbe {
    #[serde(default)]
    format: u32,
}

fn read_catalog(path: &Path, max_bytes: u64) -> Result<Option<StoredCatalog>, StoreError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(StoreError::Io(error.to_string())),
    };
    if metadata.file_type().is_symlink() {
        return Err(StoreError::Symlink);
    }
    if metadata.len() > max_bytes {
        return Err(StoreError::TooLarge { limit: max_bytes });
    }
    let file = std::fs::File::open(path).map_err(|error| StoreError::Io(error.to_string()))?;
    let mut bytes = Vec::new();
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| StoreError::Io(error.to_string()))?;
    if bytes.len() as u64 > max_bytes {
        return Err(StoreError::TooLarge { limit: max_bytes });
    }
    let format: FormatProbe =
        serde_json::from_slice(&bytes).map_err(|error| StoreError::Corrupt(error.to_string()))?;
    if format.format != StoredCatalog::FORMAT {
        return Ok(None);
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| StoreError::Corrupt(error.to_string()))
}

fn write_catalog(dir: &Path, path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    std::fs::create_dir_all(dir).map_err(|error| StoreError::Io(error.to_string()))?;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut temp = path.as_os_str().to_owned();
    temp.push(format!(".tmp.{}.{nanos}", std::process::id()));
    let temp = PathBuf::from(temp);
    let result = std::fs::write(&temp, bytes).and_then(|()| std::fs::rename(&temp, path));
    if let Err(error) = result {
        let _ = std::fs::remove_file(&temp);
        return Err(StoreError::Io(error.to_string()));
    }
    Ok(())
}

/// The registry's notion of now, for ages and cooldowns.
pub trait Clock: Send + Sync {
    /// The current time.
    fn now(&self) -> SystemTime;
}

impl<T: Clock + ?Sized> Clock for std::sync::Arc<T> {
    fn now(&self) -> SystemTime {
        (**self).now()
    }
}

/// The system wall clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
