use super::*;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn doc(dir_name: &str, document: &str) -> MaterializeEntry {
    MaterializeEntry {
        dir_name: dir_name.to_string(),
        source: MaterializeSource::Document(document.to_string()),
    }
}

fn refused(
    root: &Path,
    entries: &[MaterializeEntry],
) -> Result<MaterializeError, Box<dyn std::error::Error>> {
    match materialize_tree(root, entries) {
        Ok(report) => Err(format!("expected a refusal, wrote {report:?}").into()),
        Err(error) => Ok(error),
    }
}

#[test]
fn a_source_nested_past_the_limit_is_refused() -> TestResult {
    let temp = tempfile::tempdir()?;
    let src = temp.path().join("src");
    let mut deep = src.clone();
    for level in 0..=MAX_MATERIALIZE_DEPTH {
        deep = deep.join(format!("d{level}"));
    }
    std::fs::create_dir_all(&deep)?;
    let entry = MaterializeEntry {
        dir_name: "deep".to_string(),
        source: MaterializeSource::Dir(src),
    };

    let error = refused(&temp.path().join("out"), &[entry])?;
    assert!(
        matches!(&error, MaterializeError::TooDeep { path, max } if *path == deep && *max == MAX_MATERIALIZE_DEPTH),
        "{error:?}"
    );
    assert!(error.to_string().contains("directories deep"));
    Ok(())
}

#[test]
fn a_source_at_the_limit_is_copied() -> TestResult {
    let temp = tempfile::tempdir()?;
    let src = temp.path().join("src");
    let mut deep = src.clone();
    for level in 0..MAX_MATERIALIZE_DEPTH {
        deep = deep.join(format!("d{level}"));
    }
    std::fs::create_dir_all(&deep)?;
    std::fs::write(deep.join("leaf.md"), "leaf")?;
    let entry = MaterializeEntry {
        dir_name: "deep".to_string(),
        source: MaterializeSource::Dir(src),
    };

    let report = materialize_tree(&temp.path().join("out"), &[entry])?;
    assert_eq!(report.files, 1);
    Ok(())
}

#[test]
fn a_missing_source_directory_is_an_io_error_naming_it() -> TestResult {
    let temp = tempfile::tempdir()?;
    let missing = temp.path().join("missing");
    let entry = MaterializeEntry {
        dir_name: "gone".to_string(),
        source: MaterializeSource::Dir(missing.clone()),
    };

    let error = refused(&temp.path().join("out"), &[entry])?;
    assert!(
        matches!(&error, MaterializeError::Io { context: "reading", path, .. } if *path == missing),
        "{error:?}"
    );
    assert!(error.to_string().starts_with("reading "), "{error}");
    Ok(())
}

#[test]
fn a_regular_file_at_the_root_is_replaced_by_the_tree() -> TestResult {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("skills");
    std::fs::write(&root, "not a dir")?;

    materialize_tree(&root, &[doc("a", "x")])?;
    assert_eq!(std::fs::read_to_string(root.join("a/SKILL.md"))?, "x");
    Ok(())
}

#[test]
fn a_root_under_a_file_cannot_be_created() -> TestResult {
    let temp = tempfile::tempdir()?;
    let blocker = temp.path().join("file");
    std::fs::write(&blocker, "x")?;

    let error = refused(&blocker.join("skills"), &[])?;
    assert!(matches!(error, MaterializeError::Io { .. }), "{error:?}");
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_symlinked_root_is_unlinked_and_its_target_left_alone() -> TestResult {
    let temp = tempfile::tempdir()?;
    let target = temp.path().join("target");
    std::fs::create_dir_all(&target)?;
    std::fs::write(target.join("keep.md"), "keep")?;
    let root = temp.path().join("skills");
    std::os::unix::fs::symlink(&target, &root)?;

    materialize_tree(&root, &[doc("a", "x")])?;
    assert!(!std::fs::symlink_metadata(&root)?.file_type().is_symlink());
    assert_eq!(std::fs::read_to_string(target.join("keep.md"))?, "keep");
    assert!(!target.join("a").exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn an_entry_swapped_for_a_symlink_is_not_opened() -> TestResult {
    let temp = tempfile::tempdir()?;
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&outside)?;
    std::fs::write(outside.join("secret.txt"), "secret")?;
    let src = temp.path().join("src");
    std::fs::create_dir_all(&src)?;
    std::os::unix::fs::symlink(outside.join("secret.txt"), src.join("file"))?;
    std::os::unix::fs::symlink(&outside, src.join("dir"))?;

    let source = SourceDir::open_root(&src)?;

    assert!(source.open_file(&OsString::from("file")).is_err());
    assert!(source.open_dir(&OsString::from("dir")).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn entries_are_classified_without_following_links() -> TestResult {
    let temp = tempfile::tempdir()?;
    let src = temp.path().join("src");
    std::fs::create_dir_all(src.join("sub"))?;
    std::fs::write(src.join("a.md"), "a")?;
    std::os::unix::fs::symlink(src.join("a.md"), src.join("link"))?;

    let mut kinds: Vec<_> = SourceDir::open_root(&src)?
        .entries()?
        .into_iter()
        .map(|(name, kind)| {
            let kind = match kind {
                EntryKind::Dir => "dir",
                EntryKind::File => "file",
                EntryKind::Symlink => "symlink",
                EntryKind::Other => "other",
            };
            (name.to_string_lossy().into_owned(), kind)
        })
        .collect();
    kinds.sort_unstable();

    assert_eq!(
        kinds,
        [
            ("a.md".into(), "file"),
            ("link".into(), "symlink"),
            ("sub".into(), "dir")
        ]
    );
    Ok(())
}
