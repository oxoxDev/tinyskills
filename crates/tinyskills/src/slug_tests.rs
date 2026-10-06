use super::*;

#[test]
fn reserved_suffix_skips_other_reserved_slugs() {
    let rules = SlugRules::new()
        .max_chars(8)
        .reserved(&["draft", "draft-2"])
        .truncate(true);

    assert!(matches!(
        slugify_with("Draft", &rules),
        Ok(slug) if slug == "draft-3"
    ));
    assert!(validate_slug("draft-3", &rules).is_ok());
}

#[test]
fn reserved_suffix_fails_when_no_valid_result_fits() {
    let rules = SlugRules::new()
        .max_chars(1)
        .reserved(&["a"])
        .truncate(true);

    assert!(matches!(
        slugify_with("A", &rules),
        Err(AuthoringError::SlugTooLong { max: 1, .. })
    ));
}

#[test]
fn new_and_default_are_the_slugify_rules() {
    let rules = SlugRules::new();
    assert_eq!(rules, SlugRules::default());
    assert_eq!(rules.max_chars, MAX_NAME_LEN);
    assert!(rules.reserved.is_empty());
    assert!(!rules.truncate);
    assert_eq!(rules.punctuation, PunctuationRule::Drop);
    assert_eq!(rules.fallback, None);
}

const RESERVED: &[&str] = &["draft", "upload", "registry"];

const SEPARATED: SlugRules<'static> = SlugRules::new()
    .max_chars(64)
    .reserved(RESERVED)
    .truncate(true)
    .punctuation(PunctuationRule::Separator)
    .fallback("skill");

fn reference_separated_slugify(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    let mut prev_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }
    let capped: String = slug.chars().take(64).collect();
    let trimmed = capped.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "skill".to_string()
    } else if RESERVED.contains(&trimmed.as_str()) {
        format!("{trimmed}-2")
    } else {
        trimmed
    }
}

fn separated(name: &str) -> String {
    match slugify_with(name, &SEPARATED) {
        Ok(slug) => slug,
        Err(error) => format!("<{error}>"),
    }
}

#[test]
fn separator_rules_match_the_reference_derivation() {
    let long = "a".repeat(80);
    let dash_led = format!("-{}", "a".repeat(64));
    let cut_at_dash = format!("{}!b", "a".repeat(63));
    let cut_mid_word = format!("{} tail", "b".repeat(70));
    let names = [
        "Press Outreach",
        "  Web   Research  ",
        "rock'n'roll",
        "a.b/c\\d",
        "under_score",
        "Q3 — board pack",
        "é-x",
        "日本語",
        "",
        "!!!",
        "---",
        "Draft",
        "Upload",
        "Registry",
        "registry!",
        "draft 2",
        "UPPER lower 123",
        long.as_str(),
        dash_led.as_str(),
        cut_at_dash.as_str(),
        cut_mid_word.as_str(),
    ];
    for name in names {
        assert_eq!(
            separated(name),
            reference_separated_slugify(name),
            "{name:?}"
        );
    }
}

#[test]
fn separator_vectors() {
    assert_eq!(separated("rock'n'roll"), "rock-n-roll");
    assert_eq!(separated("a.b"), "a-b");
    assert_eq!(separated("!!!"), "skill");
    assert_eq!(separated("Draft"), "draft-2");
    assert_eq!(separated(&format!("-{}", "a".repeat(64))), "a".repeat(63));
    assert_eq!(separated(&format!("{}!b", "a".repeat(63))), "a".repeat(63));
}

#[test]
fn drop_keeps_folding_only_whitespace_dash_and_underscore() {
    let rules = SlugRules::new();
    assert!(matches!(slugify_with("rock'n'roll", &rules), Ok(slug) if slug == "rocknroll"));
    assert!(matches!(slugify_with("a.b", &rules), Ok(slug) if slug == "ab"));
    assert!(matches!(slugify_with("a_b c-d", &rules), Ok(slug) if slug == "a-b-c-d"));
}

#[test]
fn a_fallback_replaces_only_the_no_slug_refusal() {
    let rules = SlugRules::new().fallback("untitled");
    assert!(matches!(slugify_with("!!!", &rules), Ok(slug) if slug == "untitled"));
    assert!(matches!(
        slugify_with(&"a".repeat(MAX_NAME_LEN + 1), &rules),
        Err(AuthoringError::SlugTooLong { .. })
    ));
    assert!(matches!(
        slugify_with("!!!", &SlugRules::new()),
        Err(AuthoringError::NoSlug { .. })
    ));
}

#[test]
fn separator_without_truncation_refuses_a_long_name() {
    let rules = SlugRules::new()
        .max_chars(4)
        .punctuation(PunctuationRule::Separator);
    assert!(matches!(
        slugify_with("abcde", &rules),
        Err(AuthoringError::SlugTooLong { max: 4, .. })
    ));
    assert!(matches!(
        slugify_with("-ab-cd-", &rules),
        Err(AuthoringError::SlugTooLong { .. })
    ));
    assert!(matches!(slugify_with(" ab ", &rules), Ok(slug) if slug == "ab"));
    assert!(matches!(
        slugify_with("...", &rules),
        Err(AuthoringError::NoSlug { .. })
    ));
}

#[test]
fn separator_truncation_never_leaves_an_edge_dash() {
    let rules = SlugRules::new()
        .max_chars(4)
        .truncate(true)
        .punctuation(PunctuationRule::Separator);
    for name in ["ab-cd", "abc def", "a b c d e", "-abcd", "...abcd"] {
        let slug = slugify_with(name, &rules).unwrap_or_default();
        assert!(
            !slug.starts_with('-') && !slug.ends_with('-'),
            "{name:?} -> {slug:?}"
        );
        assert!(slug.len() <= 4, "{name:?} -> {slug:?}");
        assert!(!slug.is_empty(), "{name:?}");
    }
    assert!(matches!(
        slugify_with("-", &rules.max_chars(1)),
        Err(AuthoringError::NoSlug { .. })
    ));
}
