use super::*;

#[test]
fn reserved_suffix_skips_other_reserved_slugs() {
    let rules = SlugRules {
        max_chars: 8,
        reserved: &["draft", "draft-2"],
        truncate: true,
    };

    assert!(matches!(
        slugify_with("Draft", &rules),
        Ok(slug) if slug == "draft-3"
    ));
    assert!(validate_slug("draft-3", &rules).is_ok());
}

#[test]
fn reserved_suffix_fails_when_no_valid_result_fits() {
    let rules = SlugRules {
        max_chars: 1,
        reserved: &["a"],
        truncate: true,
    };

    assert!(matches!(
        slugify_with("A", &rules),
        Err(AuthoringError::SlugTooLong { max: 1, .. })
    ));
}
