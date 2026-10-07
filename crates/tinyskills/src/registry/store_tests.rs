#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;

fn catalog() -> StoredCatalog {
    let mut entry = RegistryEntry::default();
    entry.entry.id = "a".to_owned();
    entry.entry.name = "a".to_owned();
    entry.overview = "long".to_owned();
    let validators = Validators {
        etag: Some("\"e\"".to_owned()),
        ..Validators::default()
    };
    StoredCatalog::new(vec![entry], 42, validators, 3)
}

#[tokio::test]
async fn memory_store_round_trips() {
    let store = MemoryCatalogStore::new();
    assert_eq!(store.load("r").await.unwrap(), None);
    store.save("r", &catalog()).await.unwrap();
    assert_eq!(store.load("r").await.unwrap(), Some(catalog()));
    let shared = std::sync::Arc::new(store);
    assert_eq!(shared.load("r").await.unwrap(), Some(catalog()));
    shared.save("s", &catalog()).await.unwrap();
}

#[tokio::test]
async fn file_store_writes_atomically_and_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let store = FileCatalogStore::new(dir.path().join("nested"));
    assert_eq!(store.load("hermes").await.unwrap(), None);
    store.save("hermes", &catalog()).await.unwrap();
    store.save("hermes", &catalog()).await.unwrap();
    assert_eq!(store.load("hermes").await.unwrap(), Some(catalog()));
    let names: Vec<_> = std::fs::read_dir(dir.path().join("nested"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, ["hermes.json"]);
}

#[tokio::test]
async fn file_store_refuses_unsafe_ids() {
    let dir = tempfile::tempdir().unwrap();
    let store = FileCatalogStore::new(dir.path());
    for id in ["", "..", "a/b", "a b"] {
        assert_eq!(
            store.load(id).await,
            Err(StoreError::InvalidId(id.to_owned()))
        );
        assert_eq!(
            store.save(id, &catalog()).await,
            Err(StoreError::InvalidId(id.to_owned()))
        );
    }
}

#[tokio::test]
async fn file_store_reads_other_formats_as_absent_and_reports_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let store = FileCatalogStore::new(dir.path());
    let path = dir.path().join("r.json");
    std::fs::write(&path, br#"{"format": 99, "entries": []}"#).unwrap();
    assert_eq!(store.load("r").await.unwrap(), None);
    std::fs::write(&path, br#"{"entries": []}"#).unwrap();
    assert_eq!(store.load("r").await.unwrap(), None);
    std::fs::write(&path, b"{ nope").unwrap();
    assert!(matches!(store.load("r").await, Err(StoreError::Corrupt(_))));
    std::fs::write(&path, br#"{"format": 1, "entries": 5}"#).unwrap();
    assert!(matches!(store.load("r").await, Err(StoreError::Corrupt(_))));
}

#[tokio::test]
async fn file_store_caps_reads() {
    let dir = tempfile::tempdir().unwrap();
    let store = FileCatalogStore::new(dir.path()).with_max_bytes(16);
    store.save("r", &catalog()).await.unwrap();
    assert_eq!(
        store.load("r").await,
        Err(StoreError::TooLarge { limit: 16 })
    );
}

#[cfg(unix)]
#[tokio::test]
async fn file_store_refuses_symlinks() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target.json");
    std::fs::write(&target, serde_json::to_vec(&catalog()).unwrap()).unwrap();
    std::os::unix::fs::symlink(&target, dir.path().join("r.json")).unwrap();
    let store = FileCatalogStore::new(dir.path());
    assert_eq!(store.load("r").await, Err(StoreError::Symlink));
}

#[test]
fn clocks_tell_time() {
    let before = SystemTime::now();
    assert!(SystemClock.now() >= before);
    assert!(std::sync::Arc::new(SystemClock).now() >= before);
    assert_eq!(StoredCatalog::default().format, StoredCatalog::FORMAT);
}
