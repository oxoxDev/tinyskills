use super::*;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[cfg(unix)]
fn symlink_dir(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(unix)]
fn symlink_file(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink_dir(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

#[cfg(windows)]
fn symlink_file(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

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

#[test]
fn an_entry_swapped_for_a_symlink_is_not_opened() -> TestResult {
    let temp = tempfile::tempdir()?;
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&outside)?;
    std::fs::write(outside.join("secret.txt"), "secret")?;
    let src = temp.path().join("src");
    std::fs::create_dir_all(&src)?;
    symlink_file(&outside.join("secret.txt"), &src.join("file"))?;
    symlink_dir(&outside, &src.join("dir"))?;

    let source = SourceDir::open_root(&src)?;

    assert!(source.open_file(&OsString::from("file")).is_err());
    assert!(source.open_dir(&OsString::from("dir")).is_err());
    Ok(())
}

#[test]
fn entries_are_classified_without_following_links() -> TestResult {
    let temp = tempfile::tempdir()?;
    let src = temp.path().join("src");
    std::fs::create_dir_all(src.join("sub"))?;
    std::fs::write(src.join("a.md"), "a")?;
    symlink_file(&src.join("a.md"), &src.join("link"))?;

    let mut kinds = Vec::new();
    for entry in SourceDir::open_root(&src)?.entries()? {
        let (name, kind) = entry?;
        let kind = match kind {
            EntryKind::Dir => "dir",
            EntryKind::File => "file",
            EntryKind::Symlink => "symlink",
            EntryKind::Other => "other",
        };
        kinds.push((name.to_string_lossy().into_owned(), kind));
    }
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

#[test]
fn destination_entries_never_follow_a_symlink() -> TestResult {
    let temp = tempfile::tempdir()?;
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&outside)?;
    let root = temp.path().join("root");
    let tree = DestDir::replace_root(&root)?;
    symlink_dir(&outside, &root.join("dir"))?;
    symlink_file(&outside.join("target.txt"), &root.join("file"))?;

    assert!(tree.create_dir(OsStr::new("dir")).is_err());
    assert!(tree.create_file(OsStr::new("file")).is_err());
    assert!(!outside.join("target.txt").exists());

    let link = temp.path().join("link");
    symlink_dir(&root, &link)?;
    assert!(open_dir_nofollow_at(&link).is_err());
    Ok(())
}

#[test]
fn a_root_without_a_final_component_is_refused() {
    assert!(matches!(
        DestDir::replace_root(Path::new("/")),
        Err(MaterializeError::Io { .. })
    ));
}

#[test]
fn a_symlink_among_the_ancestors_is_refused_by_the_walk_but_resolved_up_front() -> TestResult {
    let temp = tempfile::tempdir()?;
    let real = temp.path().join("real");
    std::fs::create_dir_all(real.join("child"))?;
    let link = temp.path().join("link");
    symlink_dir(&real, &link)?;

    assert!(open_chain(&link.join("child")).is_err());
    assert!(open_anchored(&link.join("child"), false).is_ok());
    Ok(())
}

#[test]
fn missing_destination_ancestors_are_created_and_a_dangling_link_is_refused() -> TestResult {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("a/b/skills");

    materialize_tree(&root, &[doc("x", "doc")])?;
    assert_eq!(std::fs::read_to_string(root.join("x/SKILL.md"))?, "doc");

    let dangling = temp.path().join("dangling");
    symlink_dir(&temp.path().join("nowhere"), &dangling)?;
    let error = refused(&dangling.join("skills"), &[])?;
    assert!(matches!(error, MaterializeError::Io { .. }), "{error:?}");
    assert!(!temp.path().join("nowhere").exists());
    Ok(())
}

#[test]
fn a_relative_root_with_parent_references_resolves_through_the_filesystem() -> TestResult {
    let temp = tempfile::tempdir()?;
    let nested = temp.path().join("nested");
    std::fs::create_dir_all(&nested)?;
    let relative = nested.join("..").join("tree");

    materialize_tree(&relative, &[doc("x", "doc")])?;
    assert!(temp.path().join("tree/x/SKILL.md").is_file());

    let error = refused(&nested.join("gone/../tree"), &[])?;
    assert!(matches!(error, MaterializeError::Io { .. }), "{error:?}");
    Ok(())
}

#[test]
fn a_source_overlapping_the_destination_is_refused_before_anything_is_deleted() -> TestResult {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("skills");
    std::fs::create_dir_all(root.join("inner"))?;
    std::fs::write(root.join("inner/SKILL.md"), "keep")?;
    let bundle = |dir: &Path| MaterializeEntry {
        dir_name: "demo".to_string(),
        source: MaterializeSource::Dir(dir.to_path_buf()),
    };

    for src in [root.join("inner"), root.clone(), temp.path().to_path_buf()] {
        let error = refused(&root, &[bundle(&src)])?;
        assert!(
            matches!(&error, MaterializeError::OverlappingTrees { source_dir, .. } if *source_dir == src),
            "{error:?}"
        );
        assert!(error.to_string().contains("overlaps"));
    }
    assert_eq!(
        std::fs::read_to_string(root.join("inner/SKILL.md"))?,
        "keep"
    );

    let new_root = root.join("inner/not/yet/created");
    let error = refused(&new_root, &[bundle(&root.join("inner"))])?;
    assert!(
        matches!(error, MaterializeError::OverlappingTrees { .. }),
        "{error:?}"
    );
    Ok(())
}
