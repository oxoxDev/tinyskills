//! Slugs, rendering, and bundle scaffolding.

use std::fs;

use tinyskills::{
    AuthoringError, BundleDocument, BundleSpec, ScaffoldOptions, parse_skill_str,
    render_workflow_frontmatter, render_workflow_md, scaffold_bundle, slugify,
    validate_description, validate_display_name, yaml_scalar,
};

fn spec(slug: &str) -> BundleSpec {
    BundleSpec {
        slug: slug.to_owned(),
        description: "Does the thing.".to_owned(),
        ..BundleSpec::default()
    }
}

fn edit() -> ScaffoldOptions {
    ScaffoldOptions {
        overwrite: true,
        ..ScaffoldOptions::default()
    }
}

#[test]
fn slugify_collapses_separators_and_trims() -> Result<(), AuthoringError> {
    assert_eq!(slugify("Hello  World")?, "hello-world");
    assert_eq!(slugify("--foo__bar--")?, "foo-bar");
    assert_eq!(slugify("ALL CAPS skill!")?, "all-caps-skill");
    assert!(slugify("   ").is_err());
    assert!(slugify("!!!").is_err());
    assert!(matches!(
        slugify(&"a".repeat(65)),
        Err(AuthoringError::SlugTooLong { .. })
    ));
    Ok(())
}

#[test]
fn validation_trims_and_bounds() -> Result<(), AuthoringError> {
    assert_eq!(validate_display_name("  Name ")?, "Name");
    assert!(matches!(
        validate_display_name(" "),
        Err(AuthoringError::EmptyName)
    ));
    assert!(validate_display_name(&"a".repeat(65)).is_err());
    assert_eq!(validate_description(" d ")?, "d");
    assert!(validate_description("").is_err());
    assert!(validate_description(&"a".repeat(1025)).is_err());
    Ok(())
}

#[test]
fn yaml_scalar_quotes_only_when_needed() {
    assert_eq!(yaml_scalar("plain text"), "plain text");
    assert_eq!(yaml_scalar(""), "\"\"");
    assert_eq!(yaml_scalar("a: b"), "\"a: b\"");
    assert_eq!(yaml_scalar("- item"), "\"- item\"");
    assert_eq!(yaml_scalar("trailing "), "\"trailing \"");
    assert_eq!(
        yaml_scalar("line\nbreak \"q\" \\"),
        "\"line\\nbreak \\\"q\\\" \\\\\""
    );
}

#[test]
fn rendered_frontmatter_round_trips_through_the_parser() -> Result<(), Box<dyn std::error::Error>> {
    let spec = BundleSpec {
        slug: "demo".to_owned(),
        description: "Fix: the # thing".to_owned(),
        license: Some("MIT".to_owned()),
        author: Some("Ada".to_owned()),
        tags: vec!["a".to_owned(), "b c".to_owned()],
        allowed_tools: vec!["Bash".to_owned(), "Read".to_owned()],
    };
    let rendered = render_workflow_frontmatter(&spec);
    let (frontmatter, _, warnings) = parse_skill_str(&rendered).ok_or("not parseable")?;
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(frontmatter.name, "demo");
    assert_eq!(frontmatter.description, "Fix: the # thing");
    assert_eq!(frontmatter.license.as_deref(), Some("MIT"));
    assert_eq!(frontmatter.allowed_tools, ["Bash", "Read"]);
    assert_eq!(
        frontmatter
            .metadata
            .get("author")
            .and_then(serde_yaml::Value::as_str),
        Some("Ada")
    );

    let document = render_workflow_md(&spec);
    assert!(document.starts_with(&rendered));
    assert!(document.contains("# demo\n"));
    assert!(document.contains("## Instructions"));
    Ok(())
}

#[test]
fn minimal_frontmatter_omits_optional_blocks() {
    let rendered = render_workflow_frontmatter(&spec("demo"));
    assert_eq!(
        rendered,
        "---\nname: demo\ndescription: Does the thing.\n---\n"
    );
}

#[test]
fn scaffold_creates_document_and_resource_dirs() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("skills");
    let outcome = scaffold_bundle(&root, &spec("demo"), &ScaffoldOptions::default())?;
    assert_eq!(outcome.slug, "demo");
    assert!(outcome.document.ends_with("demo/WORKFLOW.md"));
    assert!(outcome.dir.starts_with(fs::canonicalize(&root)?));
    for sub in tinyskills::RESOURCE_DIRS {
        assert!(outcome.dir.join(sub).is_dir(), "{sub}");
    }
    let found = tinyskills::scan_root(&root, tinyskills::SkillScope::User);
    assert_eq!(found[0].dir_name, "demo");

    let as_skill = scaffold_bundle(
        &root,
        &spec("other"),
        &ScaffoldOptions {
            document: BundleDocument::Skill,
            ..ScaffoldOptions::default()
        },
    )?;
    assert!(as_skill.document.ends_with("other/SKILL.md"));
    Ok(())
}

#[test]
fn scaffold_rejects_collisions_and_missing_edit_targets() -> Result<(), Box<dyn std::error::Error>>
{
    let temp = tempfile::tempdir()?;
    scaffold_bundle(temp.path(), &spec("demo"), &ScaffoldOptions::default())?;
    let error = scaffold_bundle(temp.path(), &spec("demo"), &ScaffoldOptions::default())
        .err()
        .ok_or("collision accepted")?;
    assert!(matches!(error, AuthoringError::AlreadyExists { .. }));
    assert!(
        error
            .to_string()
            .starts_with("skill 'demo' already exists at ")
    );

    let error = scaffold_bundle(temp.path(), &spec("ghost"), &edit())
        .err()
        .ok_or("edit of missing accepted")?;
    assert!(matches!(error, AuthoringError::NotFound { .. }));
    assert!(error.to_string().contains("does not exist at"));
    Ok(())
}

#[test]
fn scaffold_rejects_unsafe_slugs_and_bad_descriptions() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    for slug in ["", ".", "..", "../x", "a/b", "a\\b", "/abs"] {
        let error = scaffold_bundle(temp.path(), &spec(slug), &ScaffoldOptions::default())
            .err()
            .ok_or("unsafe slug accepted")?;
        assert!(
            matches!(error, AuthoringError::InvalidSlug { .. }),
            "{slug}"
        );
    }
    let blank = BundleSpec {
        description: "  ".to_owned(),
        ..spec("demo")
    };
    assert!(matches!(
        scaffold_bundle(temp.path(), &blank, &ScaffoldOptions::default()),
        Err(AuthoringError::EmptyDescription)
    ));
    assert!(!temp.path().join("demo").exists());
    Ok(())
}

#[test]
fn edit_preserves_body_and_retires_the_other_document() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let dir = temp.path().join("demo");
    fs::create_dir_all(&dir)?;
    fs::write(
        dir.join("SKILL.md"),
        "---\nname: demo\ndescription: old\n---\n\nHand written instructions.\n",
    )?;
    let updated = BundleSpec {
        description: "new description".to_owned(),
        ..spec("demo")
    };
    let outcome = scaffold_bundle(temp.path(), &updated, &edit())?;
    let written = fs::read_to_string(&outcome.document)?;
    assert!(written.contains("description: new description"));
    assert!(written.ends_with("\nHand written instructions.\n"));
    assert!(!written.contains("## Instructions"));
    assert!(!dir.join("SKILL.md").exists(), "legacy document retired");
    assert!(dir.join("WORKFLOW.md").exists());
    Ok(())
}

#[test]
fn edit_refuses_to_overwrite_an_unparseable_body() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let dir = temp.path().join("broken");
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("SKILL.md"), "---\nname: broken\ndescription: x\n")?;
    let error = scaffold_bundle(temp.path(), &spec("broken"), &edit())
        .err()
        .ok_or("edit accepted")?;
    assert!(
        error
            .to_string()
            .to_lowercase()
            .contains("could not be parsed"),
        "{error}"
    );
    assert!(fs::read_to_string(dir.join("SKILL.md"))?.contains("name: broken"));
    assert!(!dir.join("WORKFLOW.md").exists());
    Ok(())
}

#[test]
fn edit_refuses_unparseable_target_even_when_fallback_is_valid() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let dir = temp.path().join("broken");
    fs::create_dir_all(&dir)?;
    // WORKFLOW.md exists but is unparseable (no closing ---)
    fs::write(
        dir.join("WORKFLOW.md"),
        "---\nname: broken\ndescription: x\n",
    )?;
    // SKILL.md is valid
    fs::write(
        dir.join("SKILL.md"),
        "---\nname: broken\ndescription: old\n---\n\nValid body.\n",
    )?;
    let error = scaffold_bundle(temp.path(), &spec("broken"), &edit())
        .err()
        .ok_or("edit accepted")?;
    assert!(
        error
            .to_string()
            .to_lowercase()
            .contains("could not be parsed"),
        "{error}"
    );
    // Both documents must remain unmodified
    assert!(fs::read_to_string(dir.join("WORKFLOW.md"))?.contains("name: broken"));
    assert!(fs::read_to_string(dir.join("SKILL.md"))?.contains("Valid body"));
    Ok(())
}

#[test]
fn edit_finds_bundles_under_legacy_roots() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let legacy = temp.path().join("legacy");
    let dir = legacy.join("old-one");
    fs::create_dir_all(&dir)?;
    fs::write(
        dir.join("SKILL.md"),
        "---\nname: old-one\ndescription: d\n---\n\nbody\n",
    )?;
    let options = ScaffoldOptions {
        overwrite: true,
        legacy_roots: vec![legacy.clone()],
        ..ScaffoldOptions::default()
    };
    let outcome = scaffold_bundle(&temp.path().join("primary"), &spec("old-one"), &options)?;
    assert_eq!(outcome.dir, fs::canonicalize(&dir)?);
    assert!(outcome.document.ends_with("old-one/WORKFLOW.md"));
    assert!(!temp.path().join("primary").join("old-one").exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn scaffold_refuses_a_symlinked_bundle_dir() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    std::os::unix::fs::symlink(outside.path(), temp.path().join("demo"))?;
    let error = scaffold_bundle(temp.path(), &spec("demo"), &edit())
        .err()
        .ok_or("symlink accepted")?;
    assert!(matches!(error, AuthoringError::SymlinkedDir { .. }));
    assert!(fs::read_dir(outside.path())?.next().is_none());
    Ok(())
}
