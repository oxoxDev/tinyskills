//! Rebuilding a skill tree from a resolved skill set.

use std::fs;
use std::path::Path;

use tinyskills::{
    DiscoveryRoot, MAX_MATERIALIZE_FILE_BYTES, MaterializeEntry, MaterializeError,
    MaterializeReport, MaterializeSource, SkillScope, discover, materialize_tree,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn document(dir_name: &str, text: &str) -> MaterializeEntry {
    MaterializeEntry {
        dir_name: dir_name.to_string(),
        source: MaterializeSource::Document(text.to_string()),
    }
}

fn bundle(dir_name: &str, src: &Path) -> MaterializeEntry {
    MaterializeEntry {
        dir_name: dir_name.to_string(),
        source: MaterializeSource::Dir(src.to_path_buf()),
    }
}

fn names(root: &Path) -> std::io::Result<Vec<String>> {
    let mut names = fs::read_dir(root)?
        .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
        .collect::<std::io::Result<Vec<_>>>()?;
    names.sort();
    Ok(names)
}

const DOC: &str = "---\nname: My Skill\ndescription: Does a thing\n---\n# body\r\n";

#[test]
fn documents_and_bundles_land_one_directory_each() -> TestResult {
    let temp = tempfile::tempdir()?;
    let src = temp.path().join("bundles/onboard");
    fs::create_dir_all(src.join("references/deep"))?;
    fs::write(
        src.join("SKILL.md"),
        "---\nname: Onboard\ndescription: Get set up\n---\n# Onboard\n",
    )?;
    fs::write(src.join("references/guide.md"), "guide")?;
    fs::write(src.join("references/deep/notes.txt"), "notes")?;
    let root = temp.path().join("ws/skills");

    let report = materialize_tree(&root, &[bundle("onboard", &src), document("my-skill", DOC)])?;

    assert_eq!(
        report,
        MaterializeReport {
            dirs: vec!["onboard".to_string(), "my-skill".to_string()],
            files: 4,
            skipped_symlinks: 0,
        }
    );
    assert_eq!(names(&root)?, ["my-skill", "onboard"]);
    assert_eq!(fs::read_to_string(root.join("my-skill/SKILL.md"))?, DOC);
    assert_eq!(
        fs::read_to_string(root.join("onboard/references/deep/notes.txt"))?,
        "notes"
    );

    let found = discover([DiscoveryRoot::new(&root, SkillScope::Project)]);
    let mut found: Vec<_> = found.iter().map(|skill| skill.name.as_str()).collect();
    found.sort_unstable();
    assert_eq!(found, ["My Skill", "Onboard"]);
    Ok(())
}

#[test]
fn every_call_rebuilds_the_tree_so_a_dropped_skill_disappears() -> TestResult {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("skills");
    materialize_tree(&root, &[document("a", DOC), document("b", DOC)])?;
    fs::write(root.join("stray.txt"), "left behind")?;

    let report = materialize_tree(
        &root,
        &[document("b", "---\nname: B\ndescription: v2\n---\n")],
    )?;

    assert_eq!(report.dirs, ["b"]);
    assert_eq!(names(&root)?, ["b"]);
    assert_eq!(
        fs::read_to_string(root.join("b/SKILL.md"))?,
        "---\nname: B\ndescription: v2\n---\n"
    );
    Ok(())
}

#[test]
fn an_empty_set_leaves_an_empty_root() -> TestResult {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("skills");
    materialize_tree(&root, &[document("a", DOC)])?;

    let report = materialize_tree(&root, &[])?;
    assert_eq!(report, MaterializeReport::default());
    assert!(root.is_dir());
    assert_eq!(names(&root)?, Vec::<String>::new());
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlinks_inside_a_bundle_are_skipped_not_followed() -> TestResult {
    let temp = tempfile::tempdir()?;
    let outside = temp.path().join("secret.txt");
    fs::write(&outside, "secret")?;
    let outside_dir = temp.path().join("outside");
    fs::create_dir_all(&outside_dir)?;
    fs::write(outside_dir.join("x.md"), "x")?;
    let src = temp.path().join("bundle");
    fs::create_dir_all(src.join("references"))?;
    fs::write(src.join("SKILL.md"), DOC)?;
    std::os::unix::fs::symlink(&outside, src.join("references/leak.txt"))?;
    std::os::unix::fs::symlink(&outside_dir, src.join("linked-dir"))?;
    let root = temp.path().join("skills");

    let report = materialize_tree(&root, &[bundle("demo", &src)])?;

    assert_eq!(report.files, 1);
    assert_eq!(report.skipped_symlinks, 2);
    assert!(!root.join("demo/references/leak.txt").exists());
    assert!(!root.join("demo/linked-dir").exists());
    assert!(root.join("demo/references").is_dir());
    Ok(())
}

#[cfg(unix)]
#[test]
fn non_regular_files_inside_a_bundle_are_not_copied() -> TestResult {
    let temp = tempfile::tempdir()?;
    let src = temp.path().join("bundle");
    fs::create_dir_all(&src)?;
    fs::write(src.join("SKILL.md"), DOC)?;
    let _listener = std::os::unix::net::UnixListener::bind(src.join("agent.sock"))?;
    let root = temp.path().join("skills");

    let report = materialize_tree(&root, &[bundle("demo", &src)])?;

    assert_eq!(report.files, 1);
    assert_eq!(report.skipped_symlinks, 0);
    assert_eq!(names(&root.join("demo"))?, ["SKILL.md"]);
    Ok(())
}

#[test]
fn an_unsafe_dir_name_is_refused_before_the_tree_is_touched() -> TestResult {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("skills");
    materialize_tree(&root, &[document("keep", DOC)])?;

    for bad in ["", ".", "..", "a/b", "a\\b", "has space", "../escape"] {
        let error = materialize_tree(&root, &[document("fine", DOC), document(bad, DOC)]);
        assert!(
            matches!(&error, Err(MaterializeError::UnsafeDirName { dir_name }) if dir_name == bad),
            "{bad:?}: {error:?}"
        );
    }
    assert_eq!(names(&root)?, ["keep"]);
    Ok(())
}

#[test]
fn a_duplicate_dir_name_is_refused_before_the_tree_is_touched() -> TestResult {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("skills");
    materialize_tree(&root, &[document("keep", DOC)])?;

    let error = materialize_tree(&root, &[document("same", DOC), document("same", DOC)]);
    assert!(
        matches!(&error, Err(MaterializeError::DuplicateDirName { dir_name }) if dir_name == "same"),
        "{error:?}"
    );
    let message = error.map_or_else(|e| e.to_string(), |_| String::new());
    assert!(message.contains("`same`"), "{message}");
    assert_eq!(names(&root)?, ["keep"]);
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_symlinked_source_directory_is_refused_and_nothing_is_copied() -> TestResult {
    let temp = tempfile::tempdir()?;
    let real = temp.path().join("real");
    fs::create_dir_all(&real)?;
    fs::write(real.join("SKILL.md"), DOC)?;
    let link = temp.path().join("link");
    std::os::unix::fs::symlink(&real, &link)?;
    let root = temp.path().join("skills");

    let error = materialize_tree(&root, &[bundle("demo", &link)]);

    assert!(
        matches!(&error, Err(MaterializeError::SymlinkedSource { path }) if *path == link),
        "{error:?}"
    );
    assert!(!root.join("demo/SKILL.md").exists());
    Ok(())
}

#[test]
fn a_source_file_over_the_size_limit_is_refused() -> TestResult {
    let temp = tempfile::tempdir()?;
    let src = temp.path().join("bundle");
    fs::create_dir_all(&src)?;
    fs::write(src.join("SKILL.md"), DOC)?;
    let big = src.join("big.bin");
    fs::File::create(&big)?.set_len(MAX_MATERIALIZE_FILE_BYTES + 1)?;
    let root = temp.path().join("skills");

    let error = materialize_tree(&root, &[bundle("demo", &src)]);

    assert!(
        matches!(&error, Err(MaterializeError::FileTooLarge { path, max }) if *path == big && *max == MAX_MATERIALIZE_FILE_BYTES),
        "{error:?}"
    );
    let message = error.map_or_else(|e| e.to_string(), |_| String::new());
    assert!(message.contains("larger than"), "{message}");
    Ok(())
}

#[test]
fn a_source_file_at_the_size_limit_is_copied() -> TestResult {
    let temp = tempfile::tempdir()?;
    let src = temp.path().join("bundle");
    fs::create_dir_all(&src)?;
    fs::File::create(src.join("edge.bin"))?.set_len(MAX_MATERIALIZE_FILE_BYTES)?;
    let root = temp.path().join("skills");

    let report = materialize_tree(&root, &[bundle("demo", &src)])?;

    assert_eq!(report.files, 1);
    assert_eq!(
        fs::metadata(root.join("demo/edge.bin"))?.len(),
        MAX_MATERIALIZE_FILE_BYTES
    );
    Ok(())
}
