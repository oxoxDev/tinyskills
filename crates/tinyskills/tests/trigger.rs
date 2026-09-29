//! Trigger pattern grammar and matching.

use tinyskills::TriggerPattern;

fn parse(raw: &str) -> Result<TriggerPattern, String> {
    TriggerPattern::parse(raw).ok_or_else(|| format!("{raw:?} did not parse"))
}

#[test]
fn parse_bare_domain() -> Result<(), String> {
    let pattern = parse("composio")?;
    assert_eq!(pattern.domain, "composio");
    assert!(pattern.event_slug.is_none());
    Ok(())
}

#[test]
fn parse_domain_with_slug() -> Result<(), String> {
    let pattern = parse("composio/trigger_received")?;
    assert_eq!(pattern.domain, "composio");
    assert_eq!(pattern.event_slug.as_deref(), Some("trigger_received"));
    Ok(())
}

#[test]
fn parse_wildcard_slug_is_bare() -> Result<(), String> {
    let pattern = parse("cron/*")?;
    assert_eq!(pattern.domain, "cron");
    assert!(pattern.event_slug.is_none());
    assert!(parse("cron/")?.event_slug.is_none());
    Ok(())
}

#[test]
fn parse_normalises_to_lowercase() -> Result<(), String> {
    let pattern = parse(" Composio/TRIGGER_RECEIVED ")?;
    assert_eq!(pattern.domain, "composio");
    assert_eq!(pattern.event_slug.as_deref(), Some("trigger_received"));
    Ok(())
}

#[test]
fn parse_rejects_blank_and_domainless() {
    assert!(TriggerPattern::parse("").is_none());
    assert!(TriggerPattern::parse("   ").is_none());
    assert!(TriggerPattern::parse("/event_slug").is_none());
}

#[test]
fn bare_domain_matches_any_event_in_domain() -> Result<(), String> {
    let pattern = parse("cron")?;
    assert!(pattern.matches("cron", "job_triggered"));
    assert!(pattern.matches("CRON", ""));
    assert!(!pattern.matches("system", "job_triggered"));
    Ok(())
}

#[test]
fn slugged_pattern_needs_the_same_slug() -> Result<(), String> {
    let pattern = parse("cron/job_triggered")?;
    assert!(pattern.matches("cron", "job_triggered"));
    assert!(pattern.matches("cron", "JOB_TRIGGERED"));
    assert!(!pattern.matches("cron", "job_failed"));
    assert!(!pattern.matches("system", "job_triggered"));
    // A host that cannot derive slugs passes "" and slugged patterns stay inert.
    assert!(!pattern.matches("cron", ""));
    Ok(())
}
