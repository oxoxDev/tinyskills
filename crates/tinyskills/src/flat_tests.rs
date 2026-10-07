use super::*;

#[test]
fn an_opening_fence_may_carry_trailing_whitespace() {
    assert_eq!(strip_fence_line("---  \nrest"), Some("rest"));
    assert_eq!(strip_fence_line("--- \t"), Some(""));
    assert_eq!(strip_fence_line("---"), Some(""));
}

#[test]
fn an_opening_fence_with_text_after_it_is_not_a_fence() {
    assert_eq!(strip_fence_line("--- name: x\n"), None);
    assert_eq!(strip_fence_line("---x"), None);
    assert_eq!(strip_fence_line("# heading\n"), None);
}

#[test]
fn a_lone_fence_line_opens_a_block_that_never_closes() {
    assert_eq!(split_frontmatter("---"), None);
    assert_eq!(split_frontmatter("---\n"), None);
}

#[test]
fn an_empty_block_splits_into_empty_frontmatter() {
    assert_eq!(split_frontmatter("---\n---\nbody"), Some(("", "body")));
}

#[test]
fn the_closing_fence_must_be_exactly_three_dashes() {
    assert_eq!(split_frontmatter("---\na: b\n ---\n----\n"), None);
    assert_eq!(
        split_frontmatter("---\na: b\n---\r\nbody\r\n"),
        Some(("a: b\n", "body\r\n"))
    );
}

#[test]
fn only_one_carriage_return_may_follow_the_closing_fence() {
    assert_eq!(split_frontmatter("---\na: b\n---\r\r\nbody\n"), None);
    assert_eq!(split_frontmatter("---\na: b\n---\r"), Some(("a: b\n", "")));
    assert_eq!(
        split_frontmatter("---\na: b\n---\r\r\n---\nbody"),
        Some(("a: b\n---\r\r\n", "body"))
    );
}

#[test]
fn missing_keys_render_in_a_readable_list() {
    let both = FlatError::MissingKeys {
        keys: vec!["name", "description"],
    };
    assert_eq!(
        both.to_string(),
        "the frontmatter is missing `name` and `description`"
    );
    let one = FlatError::MissingKeys {
        keys: vec!["description"],
    };
    assert_eq!(one.to_string(), "the frontmatter is missing `description`");
}
