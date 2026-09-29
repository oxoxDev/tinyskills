//! Portable primitives for agentskills.io-style skill bundles.
//!
//! `tinyskills` owns the host-independent parts of skill handling: document
//! parsing, metadata, deterministic discovery, collision precedence, safe
//! resource reads, and materialization of compile-time bundles. Product policy
//! such as installation roots, workspace trust, RPC, approvals, and execution
//! remains with the embedding host.

mod bundle;
mod catalog;
mod discovery;
mod document;
mod install;
mod model;
mod resource;

pub use bundle::{BundledFile, BundledSkill, InstallReport, install, is_current_materialization};
pub use catalog::{
    CatalogEntry, CatalogError, SkillsShRef, TreeMiss, catalog_entry_id, clawhub_download_url,
    closest_entry_ids, derive_download_url, download_url_from_docs_path,
    download_url_from_source_url, filter_catalog, find_catalog_entry, find_skill_md_in_tree,
    is_safe_segment, parse_catalog_json, parse_hermes_entry,
};
pub use discovery::{DiscoveryRoot, discover, load_skill_dir, resolve_collisions, scan_root};
pub use document::{inventory_resources, parse_skill, parse_skill_str};
pub use install::{
    InstallError, MAX_INSTALL_URL_LEN, derive_install_slug, is_loopback_http_url,
    is_private_or_local_host, normalize_install_url, validate_install_url, validate_resolved_host,
};
pub use model::{Skill, SkillFrontmatter, SkillScope};
pub use resource::{ResourceError, read_resource, resolve_skill};
