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
