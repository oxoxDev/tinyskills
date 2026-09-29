# tinyskills

`tinyskills` is a host-independent Rust library for agentskills.io-style skill
bundles. It parses `SKILL.md` and `WORKFLOW.md`, normalizes metadata, discovers
bundles deterministically, resolves scope collisions, inventories and safely
reads resources, and materializes skills embedded in a host binary.

The crate deliberately does not decide where a product stores skills, whether
a workspace is trusted, how skills are executed, or how changes are announced.
Embedding applications provide their roots and retain those policy decisions.

## Example

```rust
use tinyskills::{DiscoveryRoot, SkillScope, discover};

let skills = discover([
    DiscoveryRoot::new("/home/me/.agents/skills", SkillScope::User),
    DiscoveryRoot::new("./.agents/skills", SkillScope::Project),
]);

for skill in skills {
    println!("{}: {}", skill.name, skill.description);
}
```

## Capabilities

- YAML frontmatter parsing with scalar or sequence `allowed-tools`
- current `WORKFLOW.md` / `SKILL.md` and legacy `skill.json` discovery
- recursive, deterministic scans that reject symlinked directories/manifests
- explicit scope precedence for builtin, legacy, user, project, and profile roots
- traversal-, symlink-, size-, and UTF-8-safe resource reads
- validated materialization and tamper detection for compile-time bundles
- network-free registry catalog logic: Hermes catalog parsing, `SKILL.md` download-URL derivation (GitHub, ClawHub, skills.sh), entry lookup, and search filtering
- opt-in collision policies (`CollisionPolicy`, `TieBreak::LastWins`, excluded scopes) so root order can decide equal-scope ties
- authoring: `slugify`, YAML rendering, and `scaffold_bundle` (containment-checked create/edit with body preservation and resource dirs)
- defensive `remove_bundle` (slug validation, symlink rejection, canonical containment)
- fetched single-document installs: `validate_fetched_document`, `redact_url`, and an atomic `write_installed_document` with rollback
- `TriggerPattern` parsing and matching for `triggers:` frontmatter
- `read_document` for size-bounded, symlink-free, UTF-8 reads of documents and sidecars

## Development

Run from the repository root:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo build --all-targets --all-features
cargo test --all-features
```

## License

GPL-3.0-only. See [LICENSE](LICENSE).
