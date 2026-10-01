use super::*;

#[test]
fn bounded_reader_rejects_growth() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("resource");
    std::fs::write(&path, "too large")?;
    let file = std::fs::File::open(path)?;
    assert!(read_bounded_file(file, 3).is_err());
    assert!(open_resource(temp.path(), Path::new("")).is_err());
    Ok(())
}

fn skill_at(dir_name: &str, name: &str, root: &Path) -> Skill {
    Skill {
        name: name.to_owned(),
        dir_name: dir_name.to_owned(),
        location: Some(root.join("SKILL.md")),
        ..Skill::default()
    }
}

#[test]
fn resolve_skill_matches_by_display_name_and_directory_id() -> Result<(), String> {
    let root = Path::new("a");
    let skills = || [skill_at("dir", "Display", root), skill_at("x", "y", root)];
    assert_eq!(resolve_skill(skills(), "Display")?.dir_name, "dir");
    assert_eq!(resolve_skill(skills(), "dir")?.name, "Display");
    assert!(resolve_skill(skills(), "absent").is_err());
    Ok(())
}

#[test]
fn bounded_reader_returns_content_within_the_limit() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("resource");
    std::fs::write(&path, "abc")?;
    let bytes = read_bounded_file(std::fs::File::open(path)?, 3)?;
    assert_eq!(bytes, b"abc");
    Ok(())
}

#[cfg(unix)]
#[test]
fn open_resource_reports_missing_root_directory_and_leaf() -> Result<(), Box<dyn std::error::Error>>
{
    let temp = tempfile::tempdir()?;
    std::fs::create_dir(temp.path().join("dir"))?;
    assert!(open_resource(&temp.path().join("gone"), Path::new("file")).is_err());
    assert!(open_resource(temp.path(), Path::new("missing/file")).is_err());
    assert!(open_resource(temp.path(), Path::new("dir/missing")).is_err());
    std::fs::write(temp.path().join("dir/file"), "ok")?;
    assert!(open_resource(temp.path(), Path::new("dir/file")).is_ok());
    Ok(())
}

#[test]
fn read_resource_surfaces_stat_errors_for_missing_files() -> Result<(), Box<dyn std::error::Error>>
{
    let temp = tempfile::tempdir()?;
    let skill = skill_at("s", "s", temp.path());
    assert!(matches!(
        read_resource(&skill, Path::new("missing.md")),
        Err(ResourceError::Io { .. })
    ));
    Ok(())
}
