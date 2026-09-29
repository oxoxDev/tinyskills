//! Public-API tests for the network-free skill catalog logic.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::json;
use tinyskills::{
    CatalogEntry, CatalogError, SkillsShRef, TreeMiss, clawhub_download_url, derive_download_url,
    download_url_from_source_url, filter_catalog, find_catalog_entry, find_skill_md_in_tree,
    parse_catalog_json,
};

fn parse_hermes_entry(item: &serde_json::Value) -> Option<CatalogEntry> {
    tinyskills::parse_hermes_entry(item, None)
}

#[test]
fn clawhub_download_url_uses_the_file_api_and_rejects_unsafe_slugs() {
    assert_eq!(
        clawhub_download_url("apple-design").as_deref(),
        Some("https://clawhub.ai/api/v1/skills/apple-design/file?path=SKILL.md")
    );
    for slug in ["", ".", "..", "a/b", "a?b", "a b"] {
        assert_eq!(clawhub_download_url(slug), None, "slug {slug:?}");
    }
}

#[test]
fn download_url_from_source_url_rejects_non_github_and_malformed() {
    assert_eq!(
        download_url_from_source_url("https://lobehub.com/agent/x"),
        None
    );
    // GitHub URL missing the branch/path tail.
    assert_eq!(
        download_url_from_source_url("https://github.com/owner/repo"),
        None
    );
    // Unknown ref kind.
    assert_eq!(
        download_url_from_source_url("https://github.com/o/r/raw/main/x"),
        None
    );
}

#[test]
fn download_url_from_source_url_validates_segments_and_rejects_traversal() {
    // Valid blob URL.
    assert!(download_url_from_source_url("https://github.com/o/r/blob/main/x/SKILL.md").is_some());
    // Valid tree URL.
    assert!(download_url_from_source_url("https://github.com/o/r/tree/main/x/y").is_some());
    // Query string in the URL should be rejected.
    assert_eq!(
        download_url_from_source_url("https://github.com/o/r/tree/main/x?tab=readme"),
        None
    );
    // Fragment in the URL should be rejected.
    assert_eq!(
        download_url_from_source_url("https://github.com/o/r/tree/main/x#section"),
        None
    );
    // Path traversal segment (..) should be rejected.
    assert_eq!(
        download_url_from_source_url("https://github.com/o/r/tree/main/../../other/repo/main/s"),
        None
    );
    // Dot (.) segment should be rejected.
    assert_eq!(
        download_url_from_source_url("https://github.com/o/r/tree/main/./skill"),
        None
    );
    // Space in segment should be rejected.
    assert_eq!(
        download_url_from_source_url("https://github.com/o/r/tree/main/skill name"),
        None
    );
}

#[test]
fn skills_sh_ref_parses_listing_urls_only() {
    let skill = SkillsShRef::parse("https://skills.sh/getagentseal/founder-playbook/100m-leads")
        .expect("skills.sh listing");
    assert_eq!(
        (skill.owner, skill.repo, skill.skill),
        ("getagentseal", "founder-playbook", "100m-leads")
    );
    for url in [
        "https://skills.sh/owner/repo",
        "https://skills.sh/o/r/s/extra",
        "https://skills.sh/o/../s",
        "https://example.com/o/r/s",
        "https://lobehub.com/agent/x",
    ] {
        assert!(SkillsShRef::parse(url).is_none(), "not a listing: {url}");
    }
}

#[test]
fn skills_sh_candidates_cover_the_conventional_skill_dirs() {
    let skill = SkillsShRef::parse("https://skills.sh/o/r/my-skill").unwrap();
    assert_eq!(
        skill.candidate_urls(),
        vec![
            "https://raw.githubusercontent.com/o/r/HEAD/my-skill/SKILL.md",
            "https://raw.githubusercontent.com/o/r/HEAD/skills/my-skill/SKILL.md",
            "https://raw.githubusercontent.com/o/r/HEAD/.agents/skills/my-skill/SKILL.md",
            "https://raw.githubusercontent.com/o/r/HEAD/.claude/skills/my-skill/SKILL.md",
        ]
    );
}

#[test]
fn find_skill_md_in_tree_matches_the_skill_directory_at_any_depth() {
    let tree = json!({ "tree": [
        { "path": "plugins/x/skills/my-skill", "type": "tree" },
        { "path": "plugins/x/skills/not-my-skill/SKILL.md", "type": "blob" },
        { "path": "plugins/x/skills/my-skill/SKILL.md", "type": "blob" },
    ]});
    assert_eq!(
        find_skill_md_in_tree(&tree, "my-skill"),
        Ok("plugins/x/skills/my-skill/SKILL.md".to_string())
    );
    let root = json!({ "tree": [{ "path": "my-skill/SKILL.md", "type": "blob" }] });
    assert_eq!(
        find_skill_md_in_tree(&root, "my-skill"),
        Ok("my-skill/SKILL.md".to_string())
    );
    assert_eq!(
        find_skill_md_in_tree(&tree, "other-skill"),
        Err(TreeMiss::Absent)
    );
}

#[test]
fn raw_url_percent_encodes_segments_from_a_tree_listing() {
    let skill = SkillsShRef::parse("https://skills.sh/o/r/my-skill").unwrap();
    assert_eq!(
        skill.raw_url("docs #1/what?/my-skill/SKILL.md").as_deref(),
        Some("https://raw.githubusercontent.com/o/r/HEAD/docs%20%231/what%3F/my-skill/SKILL.md")
    );
}

#[test]
fn find_skill_md_in_tree_refuses_to_guess_between_same_named_directories() {
    let tree = json!({ "tree": [
        { "path": "plugins/a/my-skill/SKILL.md", "type": "blob" },
        { "path": "plugins/b/my-skill/SKILL.md", "type": "blob" },
    ]});
    assert_eq!(
        find_skill_md_in_tree(&tree, "my-skill"),
        Err(TreeMiss::Ambiguous(vec![
            "plugins/a/my-skill/SKILL.md".to_string(),
            "plugins/b/my-skill/SKILL.md".to_string(),
        ]))
    );
}

#[test]
fn find_skill_md_in_tree_does_not_trust_a_truncated_listing() {
    let truncated = json!({
        "truncated": true,
        "tree": [{ "path": "other/SKILL.md", "type": "blob" }]
    });
    assert_eq!(
        find_skill_md_in_tree(&truncated, "my-skill"),
        Err(TreeMiss::Truncated)
    );
    // A lone visible match is not proof of uniqueness: the cut-off part of the
    // listing can hold another directory with the same name.
    let one_visible = json!({
        "truncated": true,
        "tree": [{ "path": "deep/my-skill/SKILL.md", "type": "blob" }]
    });
    assert_eq!(
        find_skill_md_in_tree(&one_visible, "my-skill"),
        Err(TreeMiss::Truncated)
    );
    // Two visible matches are ambiguous whatever was cut off.
    let two_visible = json!({
        "truncated": true,
        "tree": [
            { "path": "a/my-skill/SKILL.md", "type": "blob" },
            { "path": "b/my-skill/SKILL.md", "type": "blob" },
        ]
    });
    assert!(matches!(
        find_skill_md_in_tree(&two_visible, "my-skill"),
        Err(TreeMiss::Ambiguous(_))
    ));
}
#[test]
fn parse_hermes_entry_derives_bundled_download_url_from_docs_path() {
    let item = json!({
        "name": "apple-notes",
        "description": "Manage Apple Notes",
        "category": "apple",
        "source": "built-in",
        "docsPath": "bundled/apple/apple-apple-notes",
        "tags": ["Apple"],
        "platforms": ["macos"],
        "commands": ["memo"],
        "envVars": []
    });
    let entry = parse_hermes_entry(&item).expect("entry");
    assert_eq!(
        entry.download_url,
        "https://raw.githubusercontent.com/NousResearch/hermes-agent/main/skills/apple/apple-notes/SKILL.md"
    );
}

#[test]
fn parse_hermes_entry_derives_optional_download_url_from_docs_path() {
    let item = json!({
        "name": "docker-management",
        "description": "Manage Docker",
        "category": "devops",
        "source": "optional",
        "docsPath": "optional/devops/devops-docker-management"
    });
    let entry = parse_hermes_entry(&item).expect("entry");
    assert_eq!(
        entry.download_url,
        "https://raw.githubusercontent.com/NousResearch/hermes-agent/main/optional-skills/devops/docker-management/SKILL.md"
    );
}

#[test]
fn parse_hermes_entry_derives_github_tree_source_url() {
    // NVIDIA shape: sourceUrl is a GitHub *tree* (directory) view, no
    // docsPath. The raw SKILL.md lives inside that directory. (#3741)
    let item = json!({
        "name": "aiq-deploy",
        "description": "Deploy AIQ",
        "category": "agentic-ai",
        "source": "NVIDIA",
        "docsPath": "",
        "sourceUrl": "https://github.com/NVIDIA/skills/tree/main/skills/aiq-deploy"
    });
    let entry = parse_hermes_entry(&item).expect("entry");
    assert_eq!(
        entry.download_url,
        "https://raw.githubusercontent.com/NVIDIA/skills/main/skills/aiq-deploy/SKILL.md"
    );
    assert_eq!(
        entry.source_url.as_deref(),
        Some("https://github.com/NVIDIA/skills/tree/main/skills/aiq-deploy")
    );
}

#[test]
fn parse_hermes_entry_derives_github_blob_source_url() {
    // browse.sh shape: sourceUrl is a GitHub *blob* pointing straight at the
    // SKILL.md file — rewrite host to raw, keep the path. (#3741)
    let item = json!({
        "name": "account-management",
        "description": "Account mgmt",
        "category": "account-management",
        "source": "browse.sh",
        "sourceUrl": "https://github.com/browserbase/browse.sh/blob/main/skills/plugandpay.com/account-management-ic4kjh/SKILL.md"
    });
    let entry = parse_hermes_entry(&item).expect("entry");
    assert_eq!(
        entry.download_url,
        "https://raw.githubusercontent.com/browserbase/browse.sh/main/skills/plugandpay.com/account-management-ic4kjh/SKILL.md"
    );
}

#[test]
fn parse_hermes_entry_leaves_entries_without_a_skill_md_undownloadable() {
    // LobeHub entries are system-prompt agents with no SKILL.md, and a ClawHub
    // page without a slug gives the file API nothing to fetch. download_url is
    // empty; source_url is preserved so install can point at the page. (#3741)
    for (source, url) in [
        ("LobeHub", "https://lobehub.com/agent/9-somboon"),
        ("ClawHub", "https://clawhub.ai/skills/agentkilox-code-audit"),
    ] {
        let item = json!({
            "name": "portal-skill",
            "description": "x",
            "category": "other",
            "source": source,
            "sourceUrl": url
        });
        let entry = parse_hermes_entry(&item).expect("entry");
        assert_eq!(entry.download_url, "", "no SKILL.md behind: {url}");
        assert!(!entry.has_direct_download());
        assert_eq!(entry.source_url.as_deref(), Some(url));
    }
}

#[test]
fn parse_hermes_entry_rejects_a_docs_path_segment_that_is_not_a_plain_path_segment() {
    // `docsPath` is spliced into a raw.githubusercontent URL: a reserved
    // character would change the path the URL names, so the entry gets no
    // download URL instead. (#6285)
    for docs_path in [
        "bundled/apple/apple-my skill",
        "bundled/apple/apple-my#skill",
        "bundled/ap?ple/apple-notes",
        "bundled/apple/..",
    ] {
        let entry = parse_hermes_entry(&json!({
            "name": "odd-skill",
            "description": "x",
            "category": "apple",
            "source": "built-in",
            "docsPath": docs_path
        }))
        .expect("entry");
        assert_eq!(entry.download_url, "", "docsPath {docs_path:?}");
        assert!(!entry.has_direct_download());
    }
}

#[test]
fn parse_hermes_entry_installs_clawhub_skills_by_slug() {
    // ClawHub entries carry only a slug; the file API serves its SKILL.md. (#6285)
    let entry = parse_hermes_entry(&json!({
        "name": "Apple Design",
        "description": "x",
        "category": "apple",
        "source": "ClawHub",
        "identifier": "apple-design",
        "sourceUrl": ""
    }))
    .expect("entry");
    assert_eq!(
        entry.download_url,
        "https://clawhub.ai/api/v1/skills/apple-design/file?path=SKILL.md"
    );
    assert!(entry.has_direct_download());
}

#[test]
fn parse_hermes_entry_points_skills_sh_at_the_listed_github_repo() {
    // skills.sh lists a GitHub repo's skill; install locates the file. (#6285)
    let entry = parse_hermes_entry(&json!({
        "name": "100m-leads",
        "description": "x",
        "source": "skills.sh",
        "identifier": "skills-sh/getagentseal/founder-playbook/100m-leads",
        "sourceUrl": "https://skills.sh/getagentseal/founder-playbook/100m-leads"
    }))
    .expect("entry");
    assert_eq!(
        entry.download_url,
        "https://raw.githubusercontent.com/getagentseal/founder-playbook/HEAD/100m-leads/SKILL.md"
    );
    assert_eq!(
        entry.id,
        "skills-sh/getagentseal/founder-playbook/100m-leads"
    );
}

/// Same-named entries as the live catalog has them: several `ClawHub` skills
/// share a display name, and a `ClawHub` slug equals a bundled skill's name.
fn same_named_catalog() -> Vec<CatalogEntry> {
    [
        json!({ "name": "AI Code Review", "source": "ClawHub", "identifier": "qf-code-review" }),
        json!({ "name": "AI Code Review", "source": "ClawHub", "identifier": "ai-code-review-ops" }),
        json!({ "name": "apple-notes", "source": "built-in", "docsPath": "bundled/apple/apple-apple-notes" }),
        json!({ "name": "apple-notes", "source": "ClawHub", "identifier": "apple-notes" }),
        json!({ "name": "Apple Design", "source": "ClawHub", "identifier": "apple-design" }),
    ]
    .iter()
    .map(|item| parse_hermes_entry(item).expect("entry"))
    .collect()
}

#[test]
fn parse_hermes_entry_gives_same_named_entries_distinct_ids() {
    let ids: Vec<String> = same_named_catalog().into_iter().map(|e| e.id).collect();
    assert_eq!(
        ids,
        [
            "clawhub/qf-code-review",
            "clawhub/ai-code-review-ops",
            "apple-notes",
            "clawhub/apple-notes",
            "clawhub/apple-design",
        ]
    );
}

#[test]
fn find_catalog_entry_matches_ids_and_unambiguous_legacy_names() {
    let catalog = same_named_catalog();
    let by_id = find_catalog_entry(&catalog, "clawhub/ai-code-review-ops").unwrap();
    assert_eq!(by_id.id, "clawhub/ai-code-review-ops");
    // An exact id wins over another entry carrying the same name.
    assert_eq!(
        find_catalog_entry(&catalog, "apple-notes").unwrap().source,
        "built-in"
    );
    // Ids used to be display names; a name only one entry has still resolves.
    assert_eq!(
        find_catalog_entry(&catalog, "Apple Design").unwrap().id,
        "clawhub/apple-design"
    );
}

#[test]
fn find_catalog_entry_refuses_an_ambiguous_name_and_lists_the_ids() {
    let err = find_catalog_entry(&same_named_catalog(), "AI Code Review")
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("2 catalog entries are named 'AI Code Review'"),
        "{err}"
    );
    assert!(
        err.contains("clawhub/qf-code-review") && err.contains("clawhub/ai-code-review-ops"),
        "{err}"
    );
}

#[test]
fn find_catalog_entry_not_found_suggests_real_ids_instead_of_a_refresh() {
    let error = find_catalog_entry(&same_named_catalog(), "ai-code-review").unwrap_err();
    assert!(matches!(error, CatalogError::NotFound { .. }));
    let err = error.to_string();
    assert!(
        err.starts_with("no catalog entry has id 'ai-code-review'"),
        "{err}"
    );
    assert!(err.contains("clawhub/ai-code-review-ops"), "{err}");
    assert!(!err.contains("refresh"), "{err}");
}

#[test]
fn parse_catalog_json_rejects_invalid_payloads() {
    let error = parse_catalog_json("{").expect_err("invalid json");
    assert!(error.to_string().contains("invalid catalog json"));
    assert_eq!(parse_catalog_json("[{\"name\":\"a\"}]").unwrap().len(), 1);
}

#[test]
fn derive_download_url_honours_a_non_blank_base_override() {
    let url = derive_download_url("built-in", None, "x", None, None, Some(" http://m/ "));
    assert_eq!(url, "http://m/x/SKILL.md");
    assert_eq!(
        derive_download_url("built-in", None, "x", None, None, Some("  ")),
        ""
    );
}

#[test]
fn skills_sh_tree_lookup_builds_raw_url_and_messages() {
    let skill = SkillsShRef::parse("https://skills.sh/o/r/my-skill").unwrap();
    assert_eq!(
        skill.tree_api_url(),
        "https://api.github.com/repos/o/r/git/trees/HEAD?recursive=1"
    );
    let tree = json!({ "tree": [{ "path": "x/my-skill/SKILL.md", "type": "blob" }] });
    assert_eq!(
        skill.locate_in_tree(&tree).as_deref(),
        Ok("https://raw.githubusercontent.com/o/r/HEAD/x/my-skill/SKILL.md")
    );
    let miss = skill.locate_in_tree(&json!({ "tree": [] })).unwrap_err();
    assert_eq!(miss, TreeMiss::Absent);
    assert!(
        skill
            .miss_message(&miss)
            .contains("github.com/o/r has no my-skill/SKILL.md")
    );
}

#[test]
fn filter_catalog_filters_and_puts_undownloadable_entries_last() {
    let items = [
        json!({ "name": "alpha notes", "source": "LobeHub", "sourceUrl": "https://lobehub.com/agent/a" }),
        json!({ "name": "beta notes", "source": "built-in", "category": "apple", "docsPath": "bundled/apple/apple-beta" }),
        json!({ "name": "gamma", "source": "built-in", "category": "apple", "author": "Notes Inc", "docsPath": "bundled/apple/apple-gamma" }),
    ];
    let catalog: Vec<CatalogEntry> = items.iter().filter_map(parse_hermes_entry).collect();
    let hits = filter_catalog(catalog.clone(), "NOTES", None, None);
    let names: Vec<&str> = hits.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["beta notes", "gamma", "alpha notes"]);
    let hits = filter_catalog(catalog.clone(), "", Some("BUILT-IN"), Some("Apple"));
    assert_eq!(hits.len(), 2);
    assert!(filter_catalog(catalog, "zzz", None, None).is_empty());
}

#[test]
fn catalog_entry_serde_shape_is_stable() {
    let entry = parse_hermes_entry(&json!({ "name": "n" })).unwrap();
    let value = serde_json::to_value(&entry).unwrap();
    let mut keys: Vec<&str> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "author",
            "category",
            "commands",
            "description",
            "docs_path",
            "download_url",
            "env_vars",
            "id",
            "license",
            "name",
            "platforms",
            "source",
            "source_url",
            "tags",
            "version"
        ]
    );
}

#[test]
fn not_found_without_any_resemblance_points_at_search() {
    let error = find_catalog_entry(&same_named_catalog(), "zzzz").expect_err("absent");
    assert_eq!(
        error.to_string(),
        "no catalog entry has id 'zzzz'. Use an id returned by a catalog search."
    );
}
