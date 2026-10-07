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

/// How [`slugify_with`] treats a character that is not ASCII alphanumeric.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum PunctuationRule {
    /// Whitespace, `-` and `_` fold to one `-`; every other character is
    /// dropped, so `"rock'n'roll"` becomes `rocknroll`. [`crate::slugify`]'s
    /// rule.
    #[default]
    Drop,
    /// Every run of non-alphanumeric characters folds to one `-`, so
    /// `"rock'n'roll"` becomes `rock-n-roll`.
    Separator,
}

/// A host's bounds on the slugs it will derive and accept.
///
/// Built from [`SlugRules::new`] (or [`Default`]) and the builder methods, so
/// a field added later does not break a host:
///
/// ```
/// use tinyskills::{PunctuationRule, SlugRules, slugify_with};
///
/// const RULES: SlugRules<'static> = SlugRules::new()
///     .max_chars(64)
///     .reserved(&["draft", "upload"])
///     .truncate(true)
///     .punctuation(PunctuationRule::Separator)
///     .fallback("skill");
///
/// assert_eq!(slugify_with("Draft", &RULES).ok().as_deref(), Some("draft-2"));
/// assert_eq!(slugify_with("!!!", &RULES).ok().as_deref(), Some("skill"));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct SlugRules<'a> {
    /// The longest slug, in characters. Defaults to [`MAX_NAME_LEN`].
    pub max_chars: usize,
    /// Slugs the host cannot address. Empty by default.
    pub reserved: &'a [&'a str],
    /// Whether [`slugify_with`] cuts a long name at `max_chars` rather than
    /// refusing it. Off by default, matching [`crate::slugify`].
    pub truncate: bool,
    /// How non-alphanumeric characters are treated. Defaults to
    /// [`PunctuationRule::Drop`].
    pub punctuation: PunctuationRule,
    /// The slug [`slugify_with`] returns for a name with nothing
    /// alphanumeric in it, once it passes [`validate_slug`]. `None` by default, which refuses such a name.
    pub fallback: Option<&'a str>,
}

impl<'a> SlugRules<'a> {
    /// The default rules: [`crate::slugify`]'s behaviour.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            max_chars: MAX_NAME_LEN,
            reserved: &[],
            truncate: false,
            punctuation: PunctuationRule::Drop,
            fallback: None,
        }
    }

    /// Sets [`SlugRules::max_chars`].
    #[must_use]
    pub const fn max_chars(mut self, max_chars: usize) -> Self {
        self.max_chars = max_chars;
        self
    }

    /// Sets [`SlugRules::reserved`].
    #[must_use]
    pub const fn reserved(mut self, reserved: &'a [&'a str]) -> Self {
        self.reserved = reserved;
        self
    }

    /// Sets [`SlugRules::truncate`].
    #[must_use]
    pub const fn truncate(mut self, truncate: bool) -> Self {
        self.truncate = truncate;
        self
    }

    /// Sets [`SlugRules::punctuation`].
    #[must_use]
    pub const fn punctuation(mut self, punctuation: PunctuationRule) -> Self {
        self.punctuation = punctuation;
        self
    }

    /// Sets [`SlugRules::fallback`].
    #[must_use]
    pub const fn fallback(mut self, fallback: &'a str) -> Self {
        self.fallback = Some(fallback);
        self
    }
}

impl Default for SlugRules<'_> {
    fn default() -> Self {
        Self::new()
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
/// Under [`PunctuationRule::Drop`] the derivation is [`crate::slugify`]'s, and
/// a name longer than [`SlugRules::max_chars`] is cut there (without a
/// trailing `-`) when [`SlugRules::truncate`] is set, and refused otherwise.
/// Under [`PunctuationRule::Separator`] the folded name is cut at
/// `max_chars` first and then loses any leading and trailing `-`.
///
/// A name with nothing alphanumeric in it yields [`SlugRules::fallback`]
/// when one is set, after the fallback passes [`validate_slug`] under the same
/// rules. A derived slug that lands on a reserved name
/// gets `-2` appended (or the next free number) rather than being refused:
/// deriving is authoring, and an author should not have to rename a skill to
/// dodge a route they cannot see. [`validate_slug`] is where a reserved slug
/// supplied directly is refused.
///
/// # Errors
///
/// [`AuthoringError::NoSlug`] when nothing alphanumeric remains and there is
/// no fallback, [`AuthoringError::SlugTooLong`] when the slug is too long and
/// `truncate` is off, and [`AuthoringError::InvalidSlug`] or
/// [`AuthoringError::SlugTooLong`] when the fallback itself breaks
/// [`validate_slug`].
pub fn slugify_with(name: &str, rules: &SlugRules<'_>) -> Result<String, AuthoringError> {
    let derived = match rules.punctuation {
        PunctuationRule::Drop => slug_from_name(name).and_then(|slug| bound(slug, rules)),
        PunctuationRule::Separator => separated_slug(name, rules),
    };
    let mut slug = match (derived, rules.fallback) {
        (Err(AuthoringError::NoSlug { .. }), Some(fallback)) => {
            return checked_fallback(fallback, rules);
        }
        (derived, _) => derived?,
    };
    if rules.reserved.contains(&slug.as_str()) {
        let original = slug.clone();
        for suffix in 2_u128.. {
            let ending = format!("-{suffix}");
            if ending.len() >= rules.max_chars {
                return Err(AuthoringError::SlugTooLong {
                    slug: original,
                    max: rules.max_chars,
                });
            }
            let candidate = format!("{}{}", cut(&slug, rules.max_chars - ending.len()), ending);
            if !candidate.is_empty() && !rules.reserved.contains(&candidate.as_str()) {
                slug = candidate;
                break;
            }
        }
    }
    Ok(slug)
}

fn checked_fallback(fallback: &str, rules: &SlugRules<'_>) -> Result<String, AuthoringError> {
    match validate_slug(fallback, rules) {
        Ok(()) => Ok(fallback.to_owned()),
        Err(SlugError::TooLong { max, .. }) => Err(AuthoringError::SlugTooLong {
            slug: fallback.to_owned(),
            max,
        }),
        Err(_) => Err(AuthoringError::InvalidSlug {
            slug: fallback.to_owned(),
        }),
    }
}

fn bound(slug: String, rules: &SlugRules<'_>) -> Result<String, AuthoringError> {
    if slug.len() <= rules.max_chars {
        Ok(slug)
    } else if rules.truncate {
        Ok(cut(&slug, rules.max_chars))
    } else {
        Err(AuthoringError::SlugTooLong {
            slug,
            max: rules.max_chars,
        })
    }
}

fn separated_slug(name: &str, rules: &SlugRules<'_>) -> Result<String, AuthoringError> {
    let mut folded = String::with_capacity(name.len());
    let mut prev_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            folded.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            folded.push('-');
            prev_dash = true;
        }
    }
    if rules.truncate {
        folded.truncate(rules.max_chars.min(folded.len()));
    }
    let slug = folded.trim_matches('-');
    if slug.is_empty() {
        return Err(AuthoringError::NoSlug {
            name: name.to_owned(),
        });
    }
    if slug.len() > rules.max_chars {
        return Err(AuthoringError::SlugTooLong {
            slug: slug.to_owned(),
            max: rules.max_chars,
        });
    }
    Ok(slug.to_owned())
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

#[cfg(test)]
#[path = "slug_tests.rs"]
mod tests;
