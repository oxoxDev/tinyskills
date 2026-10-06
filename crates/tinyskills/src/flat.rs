//! Flat `SKILL.md` frontmatter: one `key: value` per line, no YAML.
//!
//! [`parse_skill_str`](crate::parse_skill_str) reads frontmatter as YAML and is
//! what discovery uses. A host that stores, renders, digests, and re-serves a
//! document needs a stricter contract: the four scalars it keeps, every other
//! line verbatim, and a body that survives byte for byte. [`parse_flat`] and
//! [`render_flat`] are that contract, and `parse_flat → render_flat →
//! parse_flat` is a fixed point.

use std::fmt::Write as _;

use crate::scan::ScanDocument;

/// A `SKILL.md` document read by the flat, line-based parser.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FlatSkill {
    /// Display name, from the first `name:` line.
    pub name: String,
    /// One-line description, from the first `description:` line.
    pub description: String,
    /// Grouping category, from the first non-empty `category:` line.
    pub category: Option<String>,
    /// Publisher version, from the first non-empty `version:` line.
    pub version: Option<String>,
    /// Everything after the closing fence, verbatim: line endings and any
    /// trailing newline included.
    pub body: String,
    /// Trimmed frontmatter lines kept as no field: an unrecognised key, a
    /// line without a `:`, or a recognised key after its first occurrence.
    ///
    /// A stored document reaches an agent as written, so these lines are as
    /// visible to it as the fields are; they are kept so a scan can see them.
    pub extra_frontmatter: Vec<String>,
}

impl FlatSkill {
    /// The document's text surfaces, borrowed for [`scan_skill`](crate::scan_skill).
    #[must_use]
    pub fn scan_document(&self) -> ScanDocument<'_> {
        ScanDocument {
            name: &self.name,
            description: &self.description,
            category: self.category.as_deref(),
            version: self.version.as_deref(),
            body: &self.body,
            extra_frontmatter: &self.extra_frontmatter,
        }
    }
}

/// Why [`parse_flat`] refused a document.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum FlatError {
    /// The document does not open with a `---` fence line, or the block is
    /// never closed.
    #[error("missing a `---` frontmatter block at the top of the file")]
    MissingFrontmatter,
    /// A required key is absent or empty.
    #[error("the frontmatter is missing {}", quoted(keys))]
    MissingKeys {
        /// Each missing key, `name` before `description`.
        keys: Vec<&'static str>,
    },
}

fn quoted(keys: &[&'static str]) -> String {
    keys.iter()
        .map(|key| format!("`{key}`"))
        .collect::<Vec<_>>()
        .join(" and ")
}

/// Parses a `SKILL.md` document with the flat, line-based parser.
///
/// A leading byte-order mark is ignored. The frontmatter is the text between
/// an opening `---` line and the next line that is exactly `---` (a trailing
/// `\r` allowed). Each non-blank line is trimmed and split on its first `:`;
/// keys match case-insensitively and the first occurrence of `name`,
/// `description`, `category`, and `version` wins. An empty `category` or
/// `version` reads as absent.
///
/// # Errors
///
/// [`FlatError::MissingFrontmatter`] when there is no closed frontmatter
/// block, and [`FlatError::MissingKeys`] when `name` or `description` is
/// absent or empty.
pub fn parse_flat(src: &str) -> Result<FlatSkill, FlatError> {
    let (frontmatter, body) = split_frontmatter(src).ok_or(FlatError::MissingFrontmatter)?;

    let mut extra_frontmatter = Vec::new();
    let mut name = None;
    let mut description = None;
    let mut category = None;
    let mut version = None;
    for line in frontmatter.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            extra_frontmatter.push(line.to_string());
            continue;
        };
        let value = value.trim().to_string();
        match key.trim().to_ascii_lowercase().as_str() {
            "name" if name.is_none() => name = Some(value),
            "description" if description.is_none() => description = Some(value),
            "category" if category.is_none() => category = Some(value),
            "version" if version.is_none() => version = Some(value),
            _ => extra_frontmatter.push(line.to_string()),
        }
    }

    let present = |value: Option<String>| value.filter(|value| !value.is_empty());
    let (name, description) = match (present(name), present(description)) {
        (Some(name), Some(description)) => (name, description),
        (name, description) => {
            let keys = [
                ("name", name.is_none()),
                ("description", description.is_none()),
            ]
            .into_iter()
            .filter_map(|(key, missing)| missing.then_some(key))
            .collect();
            return Err(FlatError::MissingKeys { keys });
        }
    };

    Ok(FlatSkill {
        name,
        description,
        category: category.filter(|value| !value.is_empty()),
        version: version.filter(|value| !value.is_empty()),
        body: body.to_string(),
        extra_frontmatter,
    })
}

/// Renders a [`FlatSkill`] to `SKILL.md` source: a `---` block of `name`,
/// `description`, then `category` and `version` when set, followed by the
/// body verbatim.
///
/// Each scalar has `\n` and `\r` replaced by spaces and is then trimmed, so a
/// value can neither add a key nor close the block early. The output is the
/// canonical form, not a copy of any original source:
/// [`FlatSkill::extra_frontmatter`] is not written.
#[must_use]
pub fn render_flat(doc: &FlatSkill) -> String {
    let one_line = |s: &str| s.replace(['\n', '\r'], " ").trim().to_string();
    let mut out = String::from("---\n");
    let _ = writeln!(out, "name: {}", one_line(&doc.name));
    let _ = writeln!(out, "description: {}", one_line(&doc.description));
    if let Some(category) = &doc.category {
        let _ = writeln!(out, "category: {}", one_line(category));
    }
    if let Some(version) = &doc.version {
        let _ = writeln!(out, "version: {}", one_line(version));
    }
    out.push_str("---\n");
    out.push_str(&doc.body);
    out
}

/// Splits a document into its frontmatter text and its verbatim body.
///
/// A leading byte-order mark is ignored. The opening line must be `---`
/// followed only by whitespace; the block ends at the first line that is
/// exactly `---` (a trailing `\r` allowed). Returns `None` when either fence
/// is missing.
#[must_use]
pub fn split_frontmatter(src: &str) -> Option<(&str, &str)> {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let after_open = strip_fence_line(src)?;

    let mut offset = 0;
    for line in after_open.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            let frontmatter = &after_open[..offset];
            let body = &after_open[offset + line.len()..];
            return Some((frontmatter, body));
        }
        offset += line.len();
    }
    None
}

fn strip_fence_line(src: &str) -> Option<&str> {
    let rest = src.strip_prefix("---")?;
    match rest.find('\n') {
        Some(newline) if rest[..newline].trim().is_empty() => Some(&rest[newline + 1..]),
        None if rest.trim().is_empty() => Some(""),
        _ => None,
    }
}

#[cfg(test)]
#[path = "flat_tests.rs"]
mod tests;
