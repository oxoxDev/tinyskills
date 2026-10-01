use super::*;
use std::sync::Mutex;

static TEST_LOCK: Mutex<()> = Mutex::new(());

static FILES: &[BundledFile] = &[BundledFile {
    path: "SKILL.md",
    contents: "body",
}];

#[test]
fn reports_staging_directory_conflicts() -> Result<(), Box<dyn std::error::Error>> {
    let _lock = TEST_LOCK.lock().map_err(|_| "test lock poisoned")?;
    let root = tempfile::tempdir()?;
    let nonce = 31_001;
    TEMP_COUNTER.store(nonce, Ordering::Relaxed);
    let staging = root
        .path()
        .join(format!(".demo.tmp-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&staging)?;
    assert_eq!(
        install(
            root.path(),
            &[BundledSkill {
                dir_name: "demo",
                files: FILES
            }]
        )
        .failed
        .len(),
        1
    );
    Ok(())
}

#[test]
fn reports_backup_directory_conflicts() -> Result<(), Box<dyn std::error::Error>> {
    let _lock = TEST_LOCK.lock().map_err(|_| "test lock poisoned")?;
    let root = tempfile::tempdir()?;
    let nonce = 31_002;
    TEMP_COUNTER.store(nonce, Ordering::Relaxed);
    std::fs::create_dir(root.path().join("demo"))?;
    let backup = root
        .path()
        .join(format!(".demo.backup-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&backup)?;
    std::fs::write(backup.join("existing"), "old")?;
    assert_eq!(
        install(
            root.path(),
            &[BundledSkill {
                dir_name: "demo",
                files: FILES
            }]
        )
        .failed
        .len(),
        1
    );
    Ok(())
}
