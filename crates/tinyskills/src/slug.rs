//! Product bounds on slugs: a host's own length cap and reserved names,
//! layered on the portable rules in [`crate::slugify`].
//!
//! A slug is a directory name and, in most hosts, a URL path segment. The
//! portable rules keep it a safe one; [`SlugRules`] adds what only the host
//! knows — a tighter cap, and names its own routes already hold (a skill
//! stored as `upload` beside a static `/skills/upload` route is created and
//! then unreachable).

use crate::authoring::{AuthoringError, slug_from_name};
use crate::model::MAX_NAME_LEN;

/// A host's bounds on the slugs it will derive and accept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlugRules<'a> {
    /// The longest slug, in characters. Defaults to [`MAX_NAME_LEN`].
    pub max_chars: usize,
    /// Slugs the host cannot address. Empty by default.
    pub reserved: &'a [&'a str],
    /// Whether [`slugify_with`] cuts a long name at `max_chars` rather than
    /// refusing it. Off by default, matching [`crate::slugify`].
    pub truncate: bool,
}

impl Default for SlugRules<'_> {
    fn default() -> Self {
        Self {
            max_chars: MAX_NAME_LEN,
            reserved: &[],
            truncate: false,
        }
    }
}

/// Why [`validate_slug`] refused a slug.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SlugError {
    /// The slug is empty.
    #[error("a skill slug must not be empty")]
    Empty,
    /// The slug is not `[a-z0-9][a-z0-9-]*`.
    #[error("`{slug}` is not a valid skill slug; a slug is `[a-z0-9][a-z0-9-]*`")]
    InvalidShape {
        /// The refused slug.
        slug: String,
    },
    /// The slug is longer than [`SlugRules::max_chars`].
    #[error("that slug is {length} characters; a skill slug has to be {max} characters or fewer")]
    TooLong {
        /// Its length, in characters.
        length: usize,
        /// The cap it exceeded.
        max: usize,
    },
    /// The slug is one of [`SlugRules::reserved`].
    #[error("`{slug}` is a reserved skill slug")]
    Reserved {
        /// The refused slug.
        slug: String,
    },
}

/// Derives a slug from a display name under `rules`.
///
/// The derivation is [`crate::slugify`]'s. A name longer than
/// [`SlugRules::max_chars`] is cut there (without a trailing `-`) when
/// [`SlugRules::truncate`] is set, and refused otherwise. A derived slug that
/// lands on a reserved name gets `-2` appended rather than being refused:
/// deriving is authoring, and an author should not have to rename a skill to
/// dodge a route they cannot see. [`validate_slug`] is where a reserved slug
/// supplied directly is refused.
///
/// # Errors
///
/// [`AuthoringError::NoSlug`] when nothing alphanumeric remains, and
/// [`AuthoringError::SlugTooLong`] when the slug is too long and `truncate` is
/// off.
pub fn slugify_with(name: &str, rules: &SlugRules<'_>) -> Result<String, AuthoringError> {
    let slug = slug_from_name(name)?;
    let mut slug = if slug.len() <= rules.max_chars {
        slug
    } else if rules.truncate {
        cut(&slug, rules.max_chars)
    } else {
        return Err(AuthoringError::SlugTooLong {
            slug,
            max: rules.max_chars,
        });
    };
    if rules.reserved.contains(&slug.as_str()) {
        slug = format!("{}-2", cut(&slug, rules.max_chars.saturating_sub(2)));
    }
    Ok(slug)
}

/// Checks a slug against `rules`: a safe `[a-z0-9][a-z0-9-]*` name, within
/// [`SlugRules::max_chars`], and not reserved.
///
/// # Errors
///
/// A [`SlugError`] naming the first rule the slug breaks.
pub fn validate_slug(slug: &str, rules: &SlugRules<'_>) -> Result<(), SlugError> {
    let mut chars = slug.chars();
    let Some(first) = chars.next() else {
        return Err(SlugError::Empty);
    };
    let lower_alnum = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
    if !lower_alnum(first) || !chars.all(|c| lower_alnum(c) || c == '-') {
        return Err(SlugError::InvalidShape {
            slug: slug.to_string(),
        });
    }
    let length = slug.chars().count();
    if length > rules.max_chars {
        return Err(SlugError::TooLong {
            length,
            max: rules.max_chars,
        });
    }
    if rules.reserved.contains(&slug) {
        return Err(SlugError::Reserved {
            slug: slug.to_string(),
        });
    }
    Ok(())
}

/// The first `max` bytes of an ASCII slug, without a trailing `-`.
fn cut(slug: &str, max: usize) -> String {
    slug[..slug.len().min(max)]
        .trim_end_matches('-')
        .to_string()
}
