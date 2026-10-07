//! Cached, searchable skill registries (feature `registry`).
//!
//! A [`SkillRegistry`] owns one or more [`SkillSource`]s, keeps each source's
//! catalog in a [`CatalogStore`], and answers search, detail and facet
//! queries from an in-memory index. Stale catalogs are served while a
//! background refresh runs; concurrent refreshes of one source collapse into
//! a single fetch.
//!
//! Network I/O is host-supplied through [`RegistryTransport`]: the crate links
//! no HTTP client. Every request goes through a guard that allows `https`
//! only, resolves the host itself, rejects non-public addresses, pins the
//! connection to the addresses it checked, re-validates every redirect hop and
//! bounds every body and every operation.

mod error;
mod transport;
mod url;

pub use error::{RegistryError, RegistryErrorKind, StoreError};
pub use transport::{
    BodyChunks, BoxFuture, HttpMethod, RegistryTransport, Resolver, SystemResolver, TransportError,
    TransportRequest, TransportResponse,
};
pub use url::normalize_registry_document_url;
