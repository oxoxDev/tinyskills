#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use crate::registry::transport::{BoxFuture, TransportResponse};

struct NoTransport;

impl RegistryTransport for NoTransport {
    fn send(
        &self,
        _request: TransportRequest,
    ) -> BoxFuture<'_, Result<TransportResponse, TransportError>> {
        Box::pin(async { Err(TransportError::Connect("offline".to_owned())) })
    }
}

fn fetcher() -> GuardedFetcher {
    GuardedFetcher::new(
        Arc::new(NoTransport),
        Arc::new(crate::SystemResolver),
        FetchPolicy::default(),
        &RegistryTimeouts::default(),
        &RegistryLimits::default(),
    )
}

#[test]
fn defaults_match_the_documented_budgets() {
    let timeouts = RegistryTimeouts::default();
    assert_eq!(timeouts.catalog, Duration::from_secs(180));
    assert_eq!(timeouts.document, Duration::from_secs(15));
    let limits = RegistryLimits::default();
    assert_eq!(limits.max_document_bytes, MAX_INSTALL_DOCUMENT_BYTES as u64);
    assert_eq!(limits.max_redirects, 5);
    assert!(!FetchPolicy::default().allow_loopback_http);
    let debug = format!("{:?}", fetcher());
    assert!(debug.contains("max_redirects"));
}

#[tokio::test]
async fn literal_v6_hosts_are_checked_without_dns() {
    let pinned = fetcher()
        .check_url("https://[2606:4700::1111]/SKILL.md")
        .await
        .unwrap();
    assert_eq!(pinned, vec!["[2606:4700::1111]:443".parse().unwrap()]);
    let error = fetcher()
        .check_url("https://[fd00::1]/SKILL.md")
        .await
        .unwrap_err();
    assert_eq!(error.kind(), crate::RegistryErrorKind::UnsafeUrl);
}

#[test]
fn address_checks() {
    assert!(matches!(
        check_addresses("h", Vec::new(), false),
        Err(RegistryError::UnsafeUrl(InstallError::NoAddresses { .. }))
    ));
    let private_v6: SocketAddr = "[fe80::1]:443".parse().unwrap();
    assert!(check_addresses("h", vec![private_v6], false).is_err());
    let loopback: SocketAddr = "[::1]:80".parse().unwrap();
    assert!(check_addresses("localhost", vec![loopback], true).is_ok());
}

#[test]
fn redirect_targets_must_parse() {
    assert_eq!(
        next_hop("https://a.test/x/y", "../z").unwrap(),
        "https://a.test/z"
    );
    assert!(matches!(
        next_hop("https://a.test/", "https://[::1"),
        Err(RegistryError::Malformed {
            what: "redirect",
            ..
        })
    ));
    assert!(next_hop("not a url", "/x").is_err());
}

#[test]
fn documents_the_flat_parser_refuses_are_invalid() {
    let error = build_document(
        None,
        "https://a.test/SKILL.md",
        b"---\n\"name\": a\n\"description\": b\n---\nbody\n",
    )
    .unwrap_err();
    assert!(matches!(
        error,
        RegistryError::InvalidDocument(DocumentError::MissingField("name"))
    ));
}

#[test]
fn guarded_response_helpers() {
    let response = GuardedResponse {
        status: 503,
        url: "https://a.test/".to_owned(),
        headers: vec![("Retry-After".to_owned(), " 12 ".to_owned())],
        body: Vec::new(),
    };
    assert!(!response.is_success());
    assert_eq!(response.retry_after(), Some(Duration::from_secs(12)));
    assert!(matches!(
        response.clone().error_for_status(),
        Err(RegistryError::RateLimited { .. })
    ));
    let ok = GuardedResponse {
        status: 204,
        ..response
    };
    assert!(ok.clone().error_for_status().is_ok());
    assert_eq!(ok.header("retry-after"), Some(" 12 "));
}
