//! Flat `SKILL.md` frontmatter: one `key: value` per line, no YAML.
//!
//! [`parse_skill_str`](crate::parse_skill_str) reads frontmatter as YAML and is
//! what discovery uses. A host that stores, renders, digests, and re-serves a
//! document needs a stricter contract: the four scalars it keeps, every other
//! line kept (trimmed) for a scan, and a body that survives byte for byte.
//! [`parse_flat`] and [`render_flat`] are that contract. `parse_flat →
//! render_flat → parse_flat` is a fixed point on the parsed [`FlatSkill`]; the
//! rendered text is canonical, so blank frontmatter lines, key case, key order,
//! and whitespace around a line are not reproduced.

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
    /// Frontmatter lines kept as no field, each trimmed of surrounding
    /// whitespace and split at line endings: an unrecognised key, a line
    /// without a `:`, an empty `category:` or `version:` line, or a recognised
    /// key after its first non-empty occurrence. [`render_flat`] writes them
    /// back after the four fields.
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
/// `description`, `category`, and `version` wins; an empty `category` or
/// `version` line does not claim its key (it is kept as an extra line), so the
/// first non-empty one wins and a document with none reads as absent. Blank
/// lines are skipped.
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
            "category" if category.is_none() && !value.is_empty() => category = Some(value),
            "version" if version.is_none() && !value.is_empty() => version = Some(value),
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
        category,
        version,
        body: body.to_string(),
        extra_frontmatter,
    })
}

/// Renders a [`FlatSkill`] to `SKILL.md` source: a `---` block of `name`,
/// `description`, then `category` and `version` when set, then each
/// [`FlatSkill::extra_frontmatter`] line in stored order, followed by the body
/// verbatim.
///
/// Each scalar and extra line has `\n` and `\r` replaced by spaces and is then
/// trimmed, so a value can neither add a key nor close the block early. A
/// `category` or `version` that is empty after that is not written. An
/// extra line that is blank, is a bare `---`, or gives a value to a recognised
/// key that is not already written above it is left out, because it would
/// claim that key or end the block when parsed again. The output is the canonical form,
/// not a copy of any original source.
#[must_use]
pub fn render_flat(doc: &FlatSkill) -> String {
    let one_line = |s: &str| s.replace(['\n', '\r'], " ").trim().to_string();
    let mut out = String::from("---\n");
    let _ = writeln!(out, "name: {}", one_line(&doc.name));
    let _ = writeln!(out, "description: {}", one_line(&doc.description));
    for (key, value) in [("category", &doc.category), ("version", &doc.version)] {
        let value = value.as_deref().map(one_line).unwrap_or_default();
        if !value.is_empty() {
            let _ = writeln!(out, "{key}: {value}");
        }
    }
    for extra in &doc.extra_frontmatter {
        let line = one_line(extra);
        if !renders_as_extra(&line, doc) {
            continue;
        }
        let _ = writeln!(out, "{line}");
    }
    out.push_str("---\n");
    out.push_str(&doc.body);
    out
}

fn renders_as_extra(line: &str, doc: &FlatSkill) -> bool {
    if line.is_empty() || line == "---" {
        return false;
    }
    let Some((key, value)) = line.split_once(':') else {
        return true;
    };
    let written = |value: Option<&str>| value.is_some_and(|value| !value.trim().is_empty());
    let empty = value.trim().is_empty();
    match key.trim().to_ascii_lowercase().as_str() {
        "name" => written(Some(&doc.name)),
        "description" => written(Some(&doc.description)),
        "category" => empty || written(doc.category.as_deref()),
        "version" => empty || written(doc.version.as_deref()),
        _ => true,
    }
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
        if is_fence(line) {
            let frontmatter = &after_open[..offset];
            let body = &after_open[offset + line.len()..];
            return Some((frontmatter, body));
        }
        offset += line.len();
    }
    None
}

fn is_fence(line: &str) -> bool {
    let line = line.strip_suffix('\n').unwrap_or(line);
    line.strip_suffix('\r').unwrap_or(line) == "---"
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
