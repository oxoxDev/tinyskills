//! Intermediate-symlink handling in resource reads.

#[cfg(unix)]
#[test]
fn intermediate_symlink_components_are_rejected_with_the_symlink_error()
-> Result<(), Box<dyn std::error::Error>> {
    use std::fs;
    use std::path::Path;
    use tinyskills::{ResourceError, Skill, read_resource};

    let temp = tempfile::tempdir()?;
    let bundle = temp.path().join("bundle");
    fs::create_dir_all(bundle.join("real"))?;
    fs::write(bundle.join("real/note.txt"), "hello")?;
    fs::write(bundle.join("SKILL.md"), "---\nname: b\n---\n")?;
    // An in-bundle directory alias stays inside the root, so only the
    // per-component walk (not canonical containment) can reject it.
    std::os::unix::fs::symlink(bundle.join("real"), bundle.join("alias"))?;
    let skill = Skill {
        name: "b".to_owned(),
        location: Some(bundle.join("SKILL.md")),
        ..Skill::default()
    };
    assert_eq!(read_resource(&skill, Path::new("real/note.txt"))?, "hello");
    let error = read_resource(&skill, Path::new("alias/note.txt")).err();
    assert!(matches!(error, Some(ResourceError::Symlink)), "{error:?}");
    assert_eq!(
        error.map(|e| e.to_string()).as_deref(),
        Some("resource path is a symlink")
    );
    Ok(())
}
