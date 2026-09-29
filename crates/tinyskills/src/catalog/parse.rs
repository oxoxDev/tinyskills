//! Parsing of the aggregated Hermes catalog JSON into [`CatalogEntry`] values.

use serde_json::Value;

use super::CatalogError;
use super::download::derive_download_url;
use super::entry::CatalogEntry;

/// Parse the catalog body into its raw JSON items.
///
/// # Errors
///
/// Returns [`CatalogError::InvalidJson`] when `body` is not a JSON array.
pub fn parse_catalog_json(body: &str) -> Result<Vec<Value>, CatalogError> {
    serde_json::from_str(body).map_err(CatalogError::InvalidJson)
}

fn opt_string(item: &Value, key: &str) -> Option<String> {
    item.get(key).and_then(Value::as_str).map(str::to_string)
}

fn non_empty_string(item: &Value, key: &str) -> Option<String> {
    item.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn string_list(item: &Value, key: &str) -> Vec<String> {
    item.get(key)
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Convert one raw Hermes catalog item into a [`CatalogEntry`], deriving its
/// download URL. Returns `None` when the item has no string `name`.
///
/// `download_base_override` is forwarded to
/// [`derive_download_url`](super::derive_download_url).
#[must_use]
pub fn parse_hermes_entry(
    item: &Value,
    download_base_override: Option<&str>,
) -> Option<CatalogEntry> {
    let name = item.get("name").and_then(Value::as_str)?.to_string();
    let description = opt_string(item, "description").unwrap_or_default();
    let source = opt_string(item, "source").unwrap_or_else(|| "hermes".to_string());
    let category = opt_string(item, "category").unwrap_or_default();
    let docs_path = non_empty_string(item, "docsPath");
    let source_url = non_empty_string(item, "sourceUrl");
    let identifier = item
        .get("identifier")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let download_url = derive_download_url(
        &source,
        identifier,
        &name,
        docs_path.as_deref(),
        source_url.as_deref(),
        download_base_override,
    );

    Some(CatalogEntry {
        id: catalog_entry_id(&source, identifier, &name),
        author: opt_string(item, "author"),
        version: opt_string(item, "version"),
        license: opt_string(item, "license"),
        tags: string_list(item, "tags"),
        platforms: string_list(item, "platforms"),
        commands: string_list(item, "commands"),
        env_vars: string_list(item, "envVars"),
        name,
        description,
        source,
        category,
        download_url,
        source_url,
        docs_path,
    })
}

/// Stable, unique entry id.
///
/// Hermes publishes a unique `identifier` per entry. Most are already
/// source-qualified paths (`skills-sh/o/r/s`, `lobehub/x`, `owner/repo/path`),
/// but `ClawHub`'s is a bare slug that can equal another source's skill name, so
/// a bare identifier is prefixed with its lowercased source. Bundled and
/// optional Hermes skills carry no identifier; their names are unique among
/// themselves and contain no `/`, so they cannot collide with a qualified id.
#[must_use]
pub fn catalog_entry_id(source: &str, identifier: Option<&str>, name: &str) -> String {
    match identifier {
        Some(identifier) if identifier.contains('/') => identifier.to_string(),
        Some(slug) => format!("{}/{slug}", source.to_ascii_lowercase()),
        None => name.to_string(),
    }
}
