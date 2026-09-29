//! Fetched-document validation and atomic single-document writes.

use std::fs;

use tinyskills::{
    DocumentError, DocumentWrite, MAX_INSTALL_DOCUMENT_BYTES, WriteError, check_document_size,
    redact_url, validate_fetched_document, write_installed_document,
};

const VALID: &str = "---\nname: My Skill!\ndescription: does things\n---\n\nbody\n";

#[test]
fn redact_url_drops_userinfo_query_and_fragment() {
    assert_eq!(
        redact_url("https://user:pw@example.com:8443/a/b.md?token=s#frag"),
        "https://example.com:8443/a/b.md"
    );
    assert_eq!(
        redact_url("https://example.com/x.md"),
        "https://example.com/x.md"
    );
    assert_eq!(redact_url("not a url"), "<unparseable>");
}

#[test]
fn valid_documents_yield_frontmatter_slug_and_warnings() -> Result<(), DocumentError> {
    let document = validate_fetched_document(VALID.as_bytes())?;
    assert_eq!(document.slug, "my-skill");
    assert_eq!(document.frontmatter.description, "does things");
    assert_eq!(document.body, "\nbody\n");
    assert_eq!(document.content, VALID);
    assert!(document.warnings.is_empty());

    let with_id = "---\nname: x\ndescription: d\nmetadata:\n  id: Custom_ID\n---\n";
    assert_eq!(
        validate_fetched_document(with_id.as_bytes())?.slug,
        "custom-id"
    );

    let bad_yaml = "---\nname: [oops\n---\n";
    assert!(matches!(
        validate_fetched_document(bad_yaml.as_bytes()),
        Err(DocumentError::MissingField("name"))
    ));
    Ok(())
}

#[test]
fn invalid_documents_are_rejected_with_core_compatible_messages() {
    let oversized = vec![b'a'; MAX_INSTALL_DOCUMENT_BYTES + 1];
    let error = validate_fetched_document(&oversized).err();
    assert_eq!(
        error.map(|e| e.to_string()),
        Some(format!(
            "fetch too large: {} bytes exceeds {MAX_INSTALL_DOCUMENT_BYTES} limit",
            MAX_INSTALL_DOCUMENT_BYTES + 1
        ))
    );
    assert!(check_document_size(MAX_INSTALL_DOCUMENT_BYTES as u64).is_ok());
    assert!(check_document_size(MAX_INSTALL_DOCUMENT_BYTES as u64 + 1).is_err());

    let cases: [(&[u8], &str); 5] = [
        (&[0xff, 0xfe], "invalid SKILL.md: body is not valid utf-8"),
        (
            b"---\nname: a\ndescription: b\n",
            "invalid SKILL.md: frontmatter block opened with `---` but never terminated",
        ),
        (
            b"---\ndescription: b\n---\n",
            "invalid SKILL.md: missing required field 'name'",
        ),
        (
            b"---\nname: a\ndescription: '  '\n---\n",
            "invalid SKILL.md: missing required field 'description'",
        ),
        (
            b"---\nname: '!!!'\ndescription: d\n---\n",
            "invalid SKILL.md: cannot derive slug",
        ),
    ];
    for (bytes, expected) in cases {
        let error = validate_fetched_document(bytes)
            .err()
            .map(|e| e.to_string());
        assert!(
            error
                .as_deref()
                .is_some_and(|message| message.starts_with(expected)),
            "{expected} vs {error:?}"
        );
    }
}

#[test]
fn writes_atomically_and_is_idempotent() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("skills");
    let written = write_installed_document(&root, "demo", VALID)?;
    let DocumentWrite::Installed(path) = written else {
        return Err("expected a fresh install".into());
    };
    assert_eq!(path, root.join("demo").join("SKILL.md"));
    assert_eq!(fs::read_to_string(&path)?, VALID);
    assert!(!root.join("demo").join("SKILL.md.tmp").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o644);
    }

    let again = write_installed_document(&root, "demo", "other")?;
    assert_eq!(again, DocumentWrite::AlreadyInstalled(path.clone()));
    assert_eq!(fs::read_to_string(&path)?, VALID, "existing file untouched");
    Ok(())
}

#[test]
fn refuses_unsafe_slugs_and_incomplete_targets() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    for slug in ["", "..", "a/b", "../x", "a\\b"] {
        assert!(
            matches!(
                write_installed_document(temp.path(), slug, VALID),
                Err(WriteError::InvalidSlug(_))
            ),
            "{slug:?}"
        );
    }
    fs::create_dir_all(temp.path().join("empty"))?;
    let error = write_installed_document(temp.path(), "empty", VALID)
        .err()
        .ok_or("incomplete target accepted")?;
    assert!(
        error
            .to_string()
            .starts_with("skill install target already exists but has no SKILL.md: ")
    );
    Ok(())
}

#[test]
fn create_dir_failure_is_reported_as_typed_error() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    // A regular file where the root should be makes create_dir_all fail.
    let blocker = temp.path().join("blocker");
    fs::write(&blocker, "file")?;
    let error = write_installed_document(&blocker, "demo", VALID)
        .err()
        .ok_or("write into a file accepted")?;
    assert!(matches!(error, WriteError::CreateDir { .. }));
    assert!(
        error
            .to_string()
            .starts_with("write failed: create directory ")
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn refuses_a_symlinked_target_directory() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    std::os::unix::fs::symlink(outside.path(), temp.path().join("demo"))?;
    assert!(matches!(
        write_installed_document(temp.path(), "demo", VALID),
        Err(WriteError::Symlink(_))
    ));
    assert!(fs::read_dir(outside.path())?.next().is_none());
    Ok(())
}

#[test]
fn concurrent_installs_use_separate_temporary_files() -> Result<(), Box<dyn std::error::Error>> {
    use std::sync::Arc;
    use std::thread;

    let temp = Arc::new(tempfile::tempdir()?);
    let root = temp.path().to_path_buf();

    // Simulate two concurrent installs to the same slug.
    // Each thread attempts to write a different content.
    let mut handles = vec![];

    for (i, content) in [
        "---\nname: concurrent\ndescription: first\n---\n\nFirst body\n",
        "---\nname: concurrent\ndescription: second\n---\n\nSecond body\n",
    ]
    .iter()
    .enumerate()
    {
        let root = root.clone();
        let content = content.to_string();
        let handle = thread::spawn(move || {
            // Small delay to increase chance of concurrent execution
            if i == 0 {
                thread::sleep(std::time::Duration::from_millis(10));
            }
            write_installed_document(&root, "concurrent", &content)
        });
        handles.push(handle);
    }

    // One install should succeed (fresh), one should report already installed
    let results: Vec<_> = handles
        .into_iter()
        .map(|h| {
            h.join()
                .ok()
                .unwrap_or(Err(WriteError::InvalidSlug("join failed".into())))
        })
        .collect();

    // At least one should succeed
    assert!(results.iter().any(Result::is_ok));

    // The installed file should contain valid content, either first or second
    let path = root.join("concurrent").join("SKILL.md");
    let content = fs::read_to_string(&path)?;
    assert!(
        content.contains("First body") || content.contains("Second body"),
        "installed content should be from one of the concurrent writers"
    );

    // There should be no leftover temporary files
    let dir_contents: Vec<_> = fs::read_dir(root.join("concurrent"))?
        .filter_map(Result::ok)
        .map(|e| e.file_name().into_string().unwrap_or_default())
        .collect();
    for name in dir_contents {
        assert!(
            !name.contains("tmp"),
            "temporary file '{name}' should have been cleaned up"
        );
    }

    Ok(())
}
