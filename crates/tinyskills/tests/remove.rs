//! Safety behavior of `remove_bundle`.

use std::fs;
use std::path::{Path, PathBuf};

use tinyskills::{RemoveError, remove_bundle};

fn write(path: &Path, content: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)
}

fn error_of(roots: &[PathBuf], slug: &str) -> Result<RemoveError, Box<dyn std::error::Error>> {
    remove_bundle(roots, slug)
        .err()
        .ok_or_else(|| format!("{slug:?} was removed").into())
}

#[test]
fn rejects_path_traversal_names() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let roots = [temp.path().to_path_buf()];
    for bad in ["../etc", "foo/bar", "foo\\bar", "..", "foo/../bar", "."] {
        let error = error_of(&roots, bad)?;
        assert!(
            error.to_string().contains("path separators"),
            "{bad:?} => {error}"
        );
    }
    let long = "a".repeat(65);
    assert!(matches!(
        error_of(&roots, &long)?,
        RemoveError::NameTooLong { len: 65, max: 64 }
    ));
    Ok(())
}

#[test]
fn rejects_empty_names() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let roots = [temp.path().to_path_buf()];
    for bad in ["", "   ", "\t"] {
        let error = error_of(&roots, bad)?;
        assert!(error.to_string().contains("name is required"), "{bad:?}");
    }
    Ok(())
}

#[test]
fn missing_bundle_reports_not_installed() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let error = error_of(&[temp.path().to_path_buf()], "ghost")?;
    assert!(error.to_string().contains("not installed"), "{error}");
    assert!(
        error_of(&[], "ghost")?
            .to_string()
            .contains("not installed")
    );
    Ok(())
}

#[test]
fn refuses_directories_without_a_document() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let bogus = temp.path().join("bogus");
    write(&bogus.join("random.txt"), "not a skill")?;
    let error = error_of(&[temp.path().to_path_buf()], "bogus")?;
    assert!(
        error.to_string().contains("does not look like a workflow"),
        "{error}"
    );
    assert!(bogus.exists(), "non-skill dir must not be deleted");
    Ok(())
}

#[test]
fn removes_workflow_and_skill_documents_from_the_first_holding_root()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let first = temp.path().join("workflows");
    let second = temp.path().join("skills");
    fs::create_dir_all(&first)?;
    write(&second.join("agenty/SKILL.md"), "---\nname: agenty\n---\n")?;
    write(&first.join("wf/WORKFLOW.md"), "---\nname: wf\n---\n")?;
    let roots = [first.clone(), second.clone()];

    let removed = remove_bundle(&roots, " wf ")?;
    assert_eq!(removed, fs::canonicalize(&first)?.join("wf"));
    assert!(!first.join("wf").exists());

    remove_bundle(&roots, "agenty")?;
    assert!(!second.join("agenty").exists());
    Ok(())
}

#[test]
fn never_removes_the_root_itself() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    write(&temp.path().join("SKILL.md"), "---\nname: root\n---\n")?;
    assert!(
        error_of(&[temp.path().to_path_buf()], ".")?
            .to_string()
            .contains("separators")
    );
    assert!(temp.path().join("SKILL.md").exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn rejects_symlink_escape() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    let target = outside.path().join("real");
    write(&target.join("SKILL.md"), "---\nname: real\n---\n")?;
    std::os::unix::fs::symlink(&target, temp.path().join("real"))?;
    let error = error_of(&[temp.path().to_path_buf()], "real")?;
    assert!(error.to_string().contains("symlinked alias"), "{error}");
    assert!(target.exists(), "symlink target must not be deleted");
    Ok(())
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_alias_in_tree() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let real = temp.path().join("real");
    write(&real.join("SKILL.md"), "---\nname: real\n---\n")?;
    std::os::unix::fs::symlink(&real, temp.path().join("alias"))?;
    let error = error_of(&[temp.path().to_path_buf()], "alias")?;
    assert!(matches!(error, RemoveError::SymlinkedAlias(_)));
    assert!(real.join("SKILL.md").exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_root() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let real_root = tempfile::tempdir()?;
    write(
        &real_root.path().join("real/SKILL.md"),
        "---\nname: real\n---\n",
    )?;
    let link = temp.path().join("skills");
    std::os::unix::fs::symlink(real_root.path(), &link)?;
    let error = error_of(&[link], "real")?;
    assert!(error.to_string().contains("symlink"), "{error}");
    assert!(real_root.path().join("real/SKILL.md").exists());
    Ok(())
}

#[test]
fn refuses_a_plain_file_named_like_the_bundle() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    write(&temp.path().join("notes"), "not a bundle")?;
    let error = error_of(&[temp.path().to_path_buf()], "notes")?;
    assert!(matches!(error, RemoveError::NotADirectory(_)), "{error}");
    assert!(error.to_string().contains("is not a directory"), "{error}");
    Ok(())
}

#[test]
fn io_errors_name_the_action_and_path() {
    let error = RemoveError::Io {
        action: "remove",
        path: "/tmp/x".to_owned(),
        source: std::io::Error::other("boom"),
    };
    assert_eq!(error.to_string(), "remove /tmp/x failed: boom");
    assert!(std::error::Error::source(&error).is_some());
    assert_eq!(
        RemoveError::Escapes("/elsewhere".to_owned()).to_string(),
        "refused to remove /elsewhere — path escapes skills root"
    );
}

#[cfg(unix)]
#[test]
fn filesystem_failures_surface_as_io_errors() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir()?;
    let bundle = temp.path().join("locked");
    write(&bundle.join("SKILL.md"), "---\nname: locked\n---\n")?;
    write(&bundle.join("nested/file.txt"), "x")?;
    // A read-only subdirectory cannot have its entries unlinked.
    fs::set_permissions(bundle.join("nested"), fs::Permissions::from_mode(0o555))?;
    let result = remove_bundle(&[temp.path().to_path_buf()], "locked");
    fs::set_permissions(bundle.join("nested"), fs::Permissions::from_mode(0o755))?;
    match result {
        Err(RemoveError::Io { action, .. }) => assert_eq!(action, "remove"),
        // Privileged runners (root) bypass directory permissions.
        Ok(_) => {}
        Err(other) => return Err(format!("unexpected error: {other}").into()),
    }
    Ok(())
}
