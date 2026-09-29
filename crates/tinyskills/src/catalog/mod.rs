//! Network-free logic for skill registry catalogs.
//!
//! A catalog is the aggregated Hermes skills index (Hermes built-in and
//! optional skills, `ClawHub`, `skills.sh`, `LobeHub`, browse.sh, GitHub-hosted
//! collections). This module parses catalog JSON into [`CatalogEntry`] values,
//! derives each entry's `SKILL.md` download URL, resolves an install request to
//! one entry, and filters a catalog for search. Fetching, caching, probing
//! candidate URLs and installing stay with the embedding host.

mod download;
mod entry;
mod parse;
mod query;

use thiserror::Error;

pub use download::{
    SkillsShRef, TreeMiss, clawhub_download_url, derive_download_url, download_url_from_docs_path,
    download_url_from_source_url, find_skill_md_in_tree, is_safe_segment,
};
pub use entry::CatalogEntry;
pub use parse::{catalog_entry_id, parse_catalog_json, parse_hermes_entry};
pub use query::{closest_entry_ids, filter_catalog, find_catalog_entry};

/// Errors from parsing a catalog or resolving an entry in it.
#[derive(Debug, Error)]
pub enum CatalogError {
    /// The catalog body was not a JSON array.
    #[error("invalid catalog json: {0}")]
    InvalidJson(#[source] serde_json::Error),
    /// No entry has the requested id (or a unique matching name).
    #[error("no catalog entry has id '{id}'. {}", not_found_hint(.closest))]
    NotFound {
        /// The id that was requested.
        id: String,
        /// Up to five real ids that resemble it, best first.
        closest: Vec<String>,
    },
    /// Several entries carry the requested name, so it does not pick one.
    #[error(
        "{count} catalog entries are named '{name}'; install one by its id, e.g. {}.",
        .ids.join(", ")
    )]
    Ambiguous {
        /// The name that was requested.
        name: String,
        /// How many entries carry it.
        count: usize,
        /// Up to five of their ids.
        ids: Vec<String>,
    },
}

fn not_found_hint(closest: &[String]) -> String {
    if closest.is_empty() {
        "Use an id returned by a catalog search.".to_string()
    } else {
        format!("Closest ids: {}.", closest.join(", "))
    }
}
