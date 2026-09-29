//! Parsing and matching of the `triggers:` frontmatter patterns.
//!
//! The host owns its event types and the subscriber that reacts to matches.
//! This module only interprets the pattern grammar.

/// A parsed trigger pattern from a skill's `triggers:` frontmatter list.
///
/// Patterns take the form `domain` or `domain/event_slug`. A bare domain (or
/// `domain/*`) matches every event in that domain; with a slug only events
/// carrying the same slug match. Both parts are compared case-insensitively.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerPattern {
    /// The lowercase event domain, such as `cron` or `channel`.
    pub domain: String,
    /// Lowercase event slug; `None` matches the whole domain.
    pub event_slug: Option<String>,
}

impl TriggerPattern {
    /// Parse a raw trigger string such as `composio/trigger_received` or `cron`.
    ///
    /// Returns `None` for blank input or a slugged pattern with no domain.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }
        match raw.split_once('/') {
            Some((domain, slug)) => {
                let domain = domain.trim().to_ascii_lowercase();
                let slug = slug.trim().to_ascii_lowercase();
                if domain.is_empty() {
                    return None;
                }
                Some(Self {
                    domain,
                    event_slug: if slug.is_empty() || slug == "*" {
                        None
                    } else {
                        Some(slug)
                    },
                })
            }
            None => Some(Self {
                domain: raw.to_ascii_lowercase(),
                event_slug: None,
            }),
        }
    }

    /// Whether this pattern matches an event with the given `domain` and `slug`.
    ///
    /// A slugged pattern never matches an empty `slug`, so a host that cannot
    /// derive a slug for its events can pass `""` and slug-qualified patterns
    /// stay inert instead of firing for the entire domain.
    #[must_use]
    pub fn matches(&self, domain: &str, slug: &str) -> bool {
        if !self.domain.eq_ignore_ascii_case(domain) {
            return false;
        }
        match &self.event_slug {
            None => true,
            Some(expected) => !slug.is_empty() && expected.eq_ignore_ascii_case(slug),
        }
    }
}
