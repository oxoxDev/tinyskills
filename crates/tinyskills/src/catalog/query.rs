//! Lookup, suggestion and search over an in-memory catalog.

use super::CatalogError;
use super::entry::CatalogEntry;

/// How many alternative ids a lookup error lists.
const MAX_SUGGESTED_IDS: usize = 5;

/// Resolve an install request to exactly one catalog entry.
///
/// `entry_id` is matched against [`CatalogEntry::id`]. Ids used to be display
/// names, which many entries share, so a name is still accepted when exactly
/// one entry carries it; otherwise the error names real ids to use instead.
///
/// # Errors
///
/// Returns [`CatalogError::NotFound`] when nothing matches and
/// [`CatalogError::Ambiguous`] when several entries share the name.
pub fn find_catalog_entry<'a>(
    catalog: &'a [CatalogEntry],
    entry_id: &str,
) -> Result<&'a CatalogEntry, CatalogError> {
    let entry_id = entry_id.trim();
    if let Some(entry) = catalog.iter().find(|e| e.id == entry_id) {
        return Ok(entry);
    }
    let named: Vec<&CatalogEntry> = catalog.iter().filter(|e| e.name == entry_id).collect();
    match named.as_slice() {
        [entry] => Ok(entry),
        [] => Err(CatalogError::NotFound {
            id: entry_id.to_string(),
            closest: closest_entry_ids(catalog, entry_id),
        }),
        many => Err(CatalogError::Ambiguous {
            name: entry_id.to_string(),
            count: many.len(),
            ids: many
                .iter()
                .take(MAX_SUGGESTED_IDS)
                .map(|e| e.id.clone())
                .collect(),
        }),
    }
}

/// Ids sharing the most words with `wanted`; installable and shorter ids win
/// ties. At most five are returned.
#[must_use]
pub fn closest_entry_ids(catalog: &[CatalogEntry], wanted: &str) -> Vec<String> {
    let words: Vec<String> = wanted
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    let mut scored: Vec<(usize, bool, &str)> = catalog
        .iter()
        .filter_map(|entry| {
            let haystack = format!("{} {}", entry.id, entry.name).to_lowercase();
            let score = words
                .iter()
                .filter(|w| haystack.contains(w.as_str()))
                .count();
            (score > 0).then_some((score, entry.has_direct_download(), entry.id.as_str()))
        })
        .collect();
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then(b.1.cmp(&a.1))
            .then(a.2.len().cmp(&b.2.len()))
    });
    scored
        .into_iter()
        .take(MAX_SUGGESTED_IDS)
        .map(|(_, _, id)| id.to_string())
        .collect()
}

/// Filter and order a catalog for a search.
///
/// `source_filter` and `category_filter` match case-insensitively and exactly.
/// A non-empty `query` must appear (case-insensitively) in the name,
/// description, a tag, the category or the author. Entries that cannot be
/// downloaded directly are ordered last (stable, so match order is otherwise
/// kept), so find-and-install reaches a working hit first.
#[must_use]
pub fn filter_catalog(
    catalog: Vec<CatalogEntry>,
    query: &str,
    source_filter: Option<&str>,
    category_filter: Option<&str>,
) -> Vec<CatalogEntry> {
    let q = query.to_lowercase();
    let mut filtered: Vec<CatalogEntry> = catalog
        .into_iter()
        .filter(|entry| {
            if source_filter.is_some_and(|src| !entry.source.eq_ignore_ascii_case(src)) {
                return false;
            }
            if category_filter.is_some_and(|cat| !entry.category.eq_ignore_ascii_case(cat)) {
                return false;
            }
            q.is_empty()
                || entry.name.to_lowercase().contains(&q)
                || entry.description.to_lowercase().contains(&q)
                || entry.tags.iter().any(|t| t.to_lowercase().contains(&q))
                || entry.category.to_lowercase().contains(&q)
                || entry
                    .author
                    .as_deref()
                    .is_some_and(|a| a.to_lowercase().contains(&q))
        })
        .collect();
    filtered.sort_by_key(|entry| !entry.has_direct_download());
    filtered
}
