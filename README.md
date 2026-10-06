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
- supply-chain scan of untrusted skill text: `scan_skill` over a `ScanDocument` and its bundled files returns `pass`/`warn`/`block` findings (invisible/bidi/zero-width code points, hard-coded credentials, and escaping resource paths block; agent-addressed text, fetch-and-exec pipelines, and undeclared MCP references warn), plus `sanitize_catalogue_text` for rendering untrusted text into a prompt as data
- product slug bounds: `SlugRules` (length cap, reserved names, truncation, a `PunctuationRule` that drops or folds punctuation, and a fallback slug for a name with no alphanumerics) for `slugify_with` and `validate_slug`
- a flat, line-based `SKILL.md` parser and renderer: `parse_flat`, `render_flat`, `split_frontmatter`, and `FlatSkill::scan_document` for the scan
- `document_digest`: the sha256 of one rendered document, for pinning an installed copy (not the same value as `BundledSkill::digest`)
- authoring budgets: `validate_description_chars` (counted in characters) and `check_frontmatter_size` (bytes in the frontmatter block)
- `materialize_tree`: rebuild a `<root>/<dir>/` tree from inline documents and bundle directories, skipping symlinks (a symlinked bundle directory is refused), with checked directory names, bounded depth, a per-file size cap, and no-follow directory handles below the canonicalized ancestors of the root and each bundle directory
- `read_skill_archive` (feature `archive`): reads a `.zip`/`.skill`, `.tar`, or `.tar.gz` upload into its `SKILL.md`, root directory, and bundled files. It refuses traversal, absolute or backslash paths, symlinks and hard links, and nested archives, and checks entry-count and expanded-size caps before reading any content

## Which parser

`parse_skill_str` and `parse_skill` read frontmatter as YAML. Discovery uses
them, and they accept anything agentskills.io allows: sequences, nested
`metadata`, `allowed-tools` lists.

`parse_flat` reads one `key: value` per line and keeps only `name`,
`description`, `category`, and `version`, holding every other line, trimmed, in
`extra_frontmatter` so a scan can see it. The first non-empty `category` or
`version` wins; an empty one is kept as an extra line. Blank frontmatter lines
are discarded. Use it when a host stores, digests, and re-serves the document
itself: the body is kept byte for byte, and `render_flat` writes `name`,
`description`, a `category` and `version` only when non-empty, and then the
extra lines in stored order. `parse_flat → render_flat →
parse_flat` is a fixed point on the parsed value, not on the text: the
rendered document is canonical, so blank lines, key case, key order, and
whitespace around a line are not reproduced. `render_flat` cannot be made to
emit a second claim on a field or close the block early. A host pinning
installs with `document_digest` should digest `render_flat` output.

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
