//! The content digest of one rendered skill document.

use sha2::{Digest, Sha256};
use std::fmt::Write as _;

/// The lowercase-hex SHA-256 of one rendered `SKILL.md` document.
///
/// The digest covers the whole string — frontmatter and body — because all of
/// it reaches an agent, so a host can pin an installed document and later tell
/// whether its stored copy or its source has changed.
///
/// This is not [`BundledSkill::digest`](crate::BundledSkill::digest), which
/// hashes every file of a compiled bundle with length delimiters. The two
/// produce different values for the same `SKILL.md` and must not be compared.
#[must_use]
pub fn document_digest(document: &str) -> String {
    let digest = Sha256::digest(document.as_bytes());
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest.as_slice() {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
#[path = "digest_tests.rs"]
mod tests;
